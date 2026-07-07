use super::*;
use crate::domain::entities::InternalStatus;
use crate::domain::state_machine::transition_handler::merge_validation;
use crate::domain::state_machine::transition_handler::{
    is_merge_worktree_path, restore_task_worktree,
};
use crate::domain::state_machine::TransitionHandler;

impl<'a> TransitionHandler<'a> {
    async fn task_still_allows_execution_spawn(
        &self,
        task_id_str: &str,
        expected_status: InternalStatus,
    ) -> bool {
        let Some(task_repo) = &self.machine.context.services.task_repo else {
            return true;
        };
        let task_id = TaskId::from_string(task_id_str.to_string());
        match task_repo.get_by_id(&task_id).await {
            Ok(Some(task)) => task.internal_status == expected_status,
            Ok(None) => false,
            Err(_) => true,
        }
    }

    /// Check that the task's plan branch is still Active.
    /// Returns Err(ExecutionBlocked) if the branch is Merged or Abandoned.
    /// No-op for non-plan tasks or when repos are unavailable.
    /// Uses `execution_plan_id` (not `session_id`) to handle re-accept flows where
    /// multiple PlanBranch records exist for the same session.
    async fn check_plan_branch_active(&self, task_id_str: &str) -> Result<(), AppError> {
        use crate::domain::entities::PlanBranchStatus;

        let task_repo = match &self.machine.context.services.task_repo {
            Some(repo) => repo,
            None => return Ok(()),
        };
        let plan_branch_repo = match &self.machine.context.services.plan_branch_repo {
            Some(repo) => repo,
            None => return Ok(()),
        };

        let task_id = TaskId::from_string(task_id_str.to_string());
        let task = match task_repo.get_by_id(&task_id).await {
            Ok(Some(t)) => t,
            _ => return Ok(()),
        };

        let exec_plan_id = match &task.execution_plan_id {
            Some(id) => id,
            None => return Ok(()),
        };

        if let Ok(Some(branch)) = plan_branch_repo
            .get_by_execution_plan_id(exec_plan_id)
            .await
        {
            if !matches!(branch.status, PlanBranchStatus::Active) {
                return Err(AppError::ExecutionBlocked(format!(
                    "Plan branch '{}' is {} — cannot execute task on inactive branch",
                    branch.branch_name, branch.status
                )));
            }
        }

        Ok(())
    }

    /// Run pre-execution setup (worktree_setup + install), store log in metadata.
    /// Returns Err if setup fails in Block/AutoFix mode.
    pub(crate) async fn run_and_store_pre_execution_setup(
        &self,
        task_id_str: &str,
        project_id_str: &str,
        context: &str,
        metadata_key: &str,
    ) -> AppResult<()> {
        if let (Some(ref task_repo), Some(ref project_repo)) = (
            &self.machine.context.services.task_repo,
            &self.machine.context.services.project_repo,
        ) {
            let task_id = TaskId::from_string(task_id_str.to_string());
            let project_id = ProjectId::from_string(project_id_str.to_string());

            let task_result = task_repo.get_by_id(&task_id).await;
            let project_result = project_repo.get_by_id(&project_id).await;

            if let (Ok(Some(task)), Ok(Some(project))) = (task_result, project_result) {
                let exec_cwd = if let Some(ref wt_path) = task.worktree_path {
                    std::path::PathBuf::from(wt_path)
                } else {
                    tracing::warn!(
                        task_id = task_id_str,
                        "Skipping pre-execution setup: task has no worktree_path. \
                         Running install commands in the main repo is not safe."
                    );
                    return Ok(());
                };

                if !exec_cwd.exists() {
                    tracing::warn!(
                        task_id = task_id_str,
                        exec_cwd = %exec_cwd.display(),
                        "Execution directory does not exist, skipping pre-execution setup"
                    );
                } else if let Some(setup_result) = merge_validation::run_pre_execution_setup(
                    &project,
                    &task,
                    &exec_cwd,
                    task_id_str,
                    self.machine.context.services.event_sink.as_deref(),
                    context,
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await
                {
                    if let Ok(Some(task_updated)) = task_repo.get_by_id(&task_id).await {
                        let log_json = serde_json::to_value(&setup_result.log)
                            .unwrap_or_else(|_| serde_json::Value::Array(Vec::new()));

                        let mut metadata_obj =
                            if let Some(json_str) = task_updated.metadata.as_ref() {
                                serde_json::from_str::<serde_json::Value>(json_str)
                                    .unwrap_or_else(|_| serde_json::json!({}))
                            } else {
                                serde_json::json!({})
                            };

                        if let Some(obj) = metadata_obj.as_object_mut() {
                            obj.insert(metadata_key.to_string(), log_json);
                        }

                        if let Ok(updated_metadata) = serde_json::to_string(&metadata_obj) {
                            if let Err(e) = task_repo
                                .update_metadata(&task_id, Some(updated_metadata))
                                .await
                            {
                                tracing::warn!(task_id = %task_id, error = %e, "Failed to persist setup log metadata");
                            }
                        }
                    }

                    if !setup_result.success {
                        use crate::domain::entities::MergeValidationMode;

                        match project.merge_validation_mode {
                            MergeValidationMode::Block | MergeValidationMode::AutoFix => {
                                tracing::error!(
                                    task_id = task_id_str,
                                    "Pre-execution setup failed (install command failed). Blocking execution."
                                );
                                return Err(AppError::ExecutionBlocked(format!(
                                    "Pre-execution setup failed: install command(s) failed. Check {} in task metadata for details.",
                                    metadata_key
                                )));
                            }
                            MergeValidationMode::Warn | MergeValidationMode::Off => {
                                tracing::warn!(
                                    task_id = task_id_str,
                                    "Pre-execution setup failed (install command failed). Proceeding with warning."
                                );
                                if let Ok(Some(task_updated)) = task_repo.get_by_id(&task_id).await
                                {
                                    let mut metadata_obj =
                                        if let Some(json_str) = task_updated.metadata.as_ref() {
                                            serde_json::from_str::<serde_json::Value>(json_str)
                                                .unwrap_or_else(|_| serde_json::json!({}))
                                        } else {
                                            serde_json::json!({})
                                        };

                                    if let Some(obj) = metadata_obj.as_object_mut() {
                                        obj.insert(
                                            "execution_setup_warning".to_string(),
                                            serde_json::json!(true),
                                        );
                                    }

                                    if let Ok(updated_metadata) =
                                        serde_json::to_string(&metadata_obj)
                                    {
                                        if let Err(e) = task_repo
                                            .update_metadata(&task_id, Some(updated_metadata))
                                            .await
                                        {
                                            tracing::warn!(task_id = %task_id, error = %e, "Failed to persist setup warning metadata");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn reset_stale_steps_on_entry(&self, task_id_str: &str) {
        // Check for preserve_steps flag (set by manual failed-task restart).
        // On DB error or missing task, fall through to original reset behavior.
        if let Some(ref task_repo) = self.machine.context.services.task_repo {
            let task_id_typed = TaskId::from_string(task_id_str.to_string());
            if let Ok(Some(task)) = task_repo.get_by_id(&task_id_typed).await {
                if extract_preserve_steps(task.metadata.as_deref()) {
                    tracing::info!(
                        task_id = task_id_str,
                        "Preserving step states per manual restart flag"
                    );
                    // Clear the one-shot flag
                    let cleared = MetadataUpdate::new()
                        .with_null("preserve_steps")
                        .merge_into(task.metadata.as_deref());
                    let _ = task_repo
                        .update_metadata(&task_id_typed, Some(cleared))
                        .await;
                    // Emit step:updated so the UI refreshes the preserved step timeline
                    self.machine
                        .context
                        .services
                        .event_emitter
                        .emit("step:updated", task_id_str)
                        .await;
                    return;
                }
            }
        }

        if let Some(ref step_repo) = self.machine.context.services.step_repo {
            let task_id_typed = TaskId::from_string(task_id_str.to_string());
            match step_repo.reset_all_to_pending(&task_id_typed).await {
                Ok(count) if count > 0 => {
                    tracing::info!(
                        task_id = task_id_str,
                        count,
                        "Reset stale steps to Pending on re-entry"
                    );
                    self.machine
                        .context
                        .services
                        .event_emitter
                        .emit("step:updated", task_id_str)
                        .await;
                }
                Err(e) => {
                    tracing::warn!(
                        task_id = task_id_str,
                        error = %e,
                        "Failed to reset steps on re-entry"
                    );
                }
                _ => {}
            }
        }
    }

    async fn run_execution_freshness_check(
        &self,
        task_id_str: &str,
        project_id_str: &str,
        stage: &'static str,
    ) -> AppResult<()> {
        if let (Some(ref task_repo), Some(ref project_repo)) = (
            &self.machine.context.services.task_repo,
            &self.machine.context.services.project_repo,
        ) {
            let task_id_typed = TaskId::from_string(task_id_str.to_string());
            let project_id_typed = ProjectId::from_string(project_id_str.to_string());
            if let (Ok(Some(task)), Ok(Some(project))) = (
                task_repo.get_by_id(&task_id_typed).await,
                project_repo.get_by_id(&project_id_typed).await,
            ) {
                let repo_path = Path::new(&project.working_directory);
                let plan_branch = get_task_plan_branch(
                    &task,
                    &project,
                    &self.machine.context.services.plan_branch_repo,
                    &self.machine.context.services.task_repo,
                )
                .await;
                let config = reconciliation_config();
                let event_sink = self.machine.context.services.event_sink.as_deref();
                let activity_event_repo =
                    self.machine.context.services.activity_event_repo.as_ref();
                let freshness_result = freshness::ensure_branches_fresh(
                    repo_path,
                    &task,
                    &project,
                    task_id_str,
                    plan_branch
                        .as_ref()
                        .map(|branch| branch.branch_name.as_str()),
                    plan_branch
                        .as_ref()
                        .map(|branch| branch.source_branch.as_str()),
                    event_sink,
                    activity_event_repo,
                    stage,
                    config,
                )
                .await;
                apply_freshness_result(freshness_result, &task, task_id_str, task_repo).await?;
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn ensure_executing_branch_and_worktree(
        &self,
        task_id_str: &str,
        project_id_str: &str,
    ) -> AppResult<()> {
        if let (Some(ref task_repo), Some(ref project_repo)) = (
            &self.machine.context.services.task_repo,
            &self.machine.context.services.project_repo,
        ) {
            let task_id = TaskId::from_string(task_id_str.to_string());
            let project_id = ProjectId::from_string(project_id_str.to_string());

            let task_result = task_repo.get_by_id(&task_id).await;
            let project_result = project_repo.get_by_id(&project_id).await;

            if let (Ok(Some(mut task)), Ok(Some(project))) = (task_result, project_result) {
                let repo_path = Path::new(&project.working_directory);
                let plan_branch_repo = &self.machine.context.services.plan_branch_repo;
                let task_repo_ref = &self.machine.context.services.task_repo;
                let pr_creation_guard_ref = &self.machine.context.services.pr_creation_guard;
                let github_service_ref = &self.machine.context.services.github_service;
                let plan_pr_description_drafter_ref =
                    &self.machine.context.services.plan_pr_description_drafter;

                if task
                    .worktree_path
                    .as_deref()
                    .map(is_merge_worktree_path)
                    .unwrap_or(false)
                {
                    let stale_path = task.worktree_path.clone().unwrap_or_default();
                    match restore_task_worktree(&mut task, &project, repo_path).await {
                        Ok(restored) => {
                            task.touch();
                            tracing::info!(
                                task_id = task_id_str,
                                restored_path = %restored.display(),
                                stale_path,
                                "Restored stale merge worktree on execution entry"
                            );
                            if let Err(e) = task_repo.update(&task).await {
                                tracing::error!(
                                    task_id = task_id_str,
                                    error = %e,
                                    "Failed to persist restored execution worktree_path"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                task_id = task_id_str,
                                error = %e,
                                stale_path,
                                "Failed to restore stale merge worktree on execution entry — clearing worktree_path for recreation"
                            );
                            task.worktree_path = None;
                            task.touch();
                            if let Err(update_err) = task_repo.update(&task).await {
                                tracing::error!(
                                    task_id = task_id_str,
                                    error = %update_err,
                                    "Failed to clear stale execution worktree_path"
                                );
                            }
                        }
                    }
                }

                let mut branch_self_healed = false;
                if let Some(ref branch) = task.task_branch.clone() {
                    let branch_exists = GitService::branch_exists(repo_path, branch)
                        .await
                        .unwrap_or(false);
                    if !branch_exists {
                        tracing::warn!(
                            task_id = task_id_str,
                            branch = %branch,
                            "Stale task_branch detected — branch deleted, self-healing by creating fresh branch"
                        );
                        if let Some(ref stored_wt) = task.worktree_path.clone() {
                            let stored = std::path::PathBuf::from(stored_wt);
                            if stored.exists() {
                                let _ = GitService::delete_worktree(repo_path, &stored).await;
                            }
                        }
                        let expected_wt_path_str =
                            compute_task_worktree_path(&project, task_id_str);
                        let expected_wt_path = std::path::PathBuf::from(&expected_wt_path_str);
                        if expected_wt_path.exists() {
                            let _ = GitService::delete_worktree(repo_path, &expected_wt_path).await;
                        }
                        task.task_branch = None;
                        task.worktree_path = None;
                        task.merge_commit_sha = None;
                        task.touch();
                        if let Err(e) = task_repo.update(&task).await {
                            tracing::error!(
                                task_id = task_id_str,
                                error = %e,
                                "Failed to clear stale git refs during self-heal"
                            );
                        }
                        match create_fresh_branch_and_worktree(
                            &task,
                            &project,
                            task_id_str,
                            repo_path,
                            plan_branch_repo,
                            task_repo_ref,
                            pr_creation_guard_ref,
                            github_service_ref,
                            plan_pr_description_drafter_ref,
                        )
                        .await
                        {
                            Ok((new_branch, new_worktree)) => {
                                task.task_branch = Some(new_branch.clone());
                                task.worktree_path =
                                    Some(new_worktree.to_string_lossy().to_string());
                                task.touch();
                                tracing::info!(
                                    task_id = task_id_str,
                                    branch = %new_branch,
                                    worktree_path = %new_worktree.display(),
                                    "Self-healed: created fresh branch and worktree for deleted branch"
                                );
                                if let Err(e) = task_repo.update(&task).await {
                                    tracing::error!(
                                        task_id = task_id_str,
                                        error = %e,
                                        "Failed to persist self-healed branch info"
                                    );
                                }
                                branch_self_healed = true;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                }

                if !branch_self_healed {
                    if task.task_branch.is_none() {
                        match create_fresh_branch_and_worktree(
                            &task,
                            &project,
                            task_id_str,
                            repo_path,
                            plan_branch_repo,
                            task_repo_ref,
                            pr_creation_guard_ref,
                            github_service_ref,
                            plan_pr_description_drafter_ref,
                        )
                        .await
                        {
                            Ok((branch_name, worktree_path)) => {
                                tracing::info!(
                                    task_id = task_id_str,
                                    branch = %branch_name,
                                    worktree_path = %worktree_path.display(),
                                    "Created worktree with task branch"
                                );
                                task.task_branch = Some(branch_name);
                                task.worktree_path =
                                    Some(worktree_path.to_string_lossy().to_string());
                                task.touch();
                                if let Err(e) = task_repo.update(&task).await {
                                    tracing::error!(error = %e, "Failed to persist task branch info");
                                }
                            }
                            Err(e) => return Err(e),
                        }
                    }

                    if let Ok(Some(mut task)) = task_repo.get_by_id(&task_id).await {
                        if let Some(ref branch) = task.task_branch.clone() {
                            let expected_wt_path =
                                compute_task_worktree_path(&project, task_id_str);
                            let expected_wt_buf = std::path::PathBuf::from(&expected_wt_path);
                            let stored_path_exists = task
                                .worktree_path
                                .as_ref()
                                .map(|p| std::path::PathBuf::from(p).exists())
                                .unwrap_or(false);
                            let expected_path_exists = expected_wt_buf.exists();
                            if !stored_path_exists && !expected_path_exists {
                                let branch_exists = GitService::branch_exists(repo_path, branch)
                                    .await
                                    .unwrap_or(false);
                                if !branch_exists {
                                    return Err(AppError::ExecutionBlocked(format!(
                                        "{}: branch '{}' no longer exists (deleted during prior merge cleanup). Task needs manual recovery or reset to Ready.",
                                        GIT_ISOLATION_ERROR_PREFIX, branch
                                    )));
                                }
                                tracing::info!(
                                    task_id = task_id_str,
                                    branch = %branch,
                                    expected_wt = %expected_wt_path,
                                    "Worktree missing for task with existing branch — re-creating"
                                );
                                match GitService::checkout_existing_branch_worktree(
                                    repo_path,
                                    &expected_wt_buf,
                                    branch,
                                )
                                .await
                                {
                                    Ok(_) => {
                                        task.worktree_path = Some(expected_wt_path);
                                        task.touch();
                                        if let Err(e) = task_repo.update(&task).await {
                                            tracing::error!(
                                                error = %e,
                                                "Failed to persist re-created worktree_path"
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        return Err(AppError::ExecutionBlocked(format!(
                                            "{}: could not re-create missing worktree for task with existing branch: {}",
                                            GIT_ISOLATION_ERROR_PREFIX, e
                                        )));
                                    }
                                }
                            } else if !stored_path_exists && expected_path_exists {
                                task.worktree_path = Some(expected_wt_path);
                                task.touch();
                                if let Err(e) = task_repo.update(&task).await {
                                    tracing::error!(
                                        error = %e,
                                        "Failed to update stale worktree_path in DB"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn build_execution_prompt(&self, task_id_str: &str, base_prompt: String) -> String {
        let mut prompt = base_prompt;
        if let Some(ref task_repo) = self.machine.context.services.task_repo {
            let task_id_typed = TaskId::from_string(task_id_str.to_string());
            if let Ok(Some(task)) = task_repo.get_by_id(&task_id_typed).await {
                if let Some(note) = extract_restart_note(task.metadata.as_deref()) {
                    prompt = format!("{}\n\nUser note: {}", prompt, note);
                    let cleared = MetadataUpdate::new()
                        .with_null("restart_note")
                        .merge_into(task.metadata.as_deref());
                    if let Err(e) = task_repo
                        .update_metadata(&task_id_typed, Some(cleared))
                        .await
                    {
                        tracing::warn!(
                            task_id = task_id_str,
                            error = %e,
                            "Failed to clear restart_note from metadata"
                        );
                    }
                }
            }
        }
        prompt
    }

    async fn send_task_execution_message(
        &self,
        task_id_str: &str,
        prompt: &str,
        failure_log: &str,
    ) -> AppResult<()> {
        match self
            .machine
            .context
            .services
            .chat_service
            .send_message(
                crate::domain::entities::ChatContextType::TaskExecution,
                task_id_str,
                prompt,
                Default::default(),
            )
            .await
        {
            Ok(result) if result.was_queued => {
                tracing::info!(
                    task_id = task_id_str,
                    "Agent already running for this task — treating on_enter as no-op"
                );
                Ok(())
            }
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::error!(
                    task_id = task_id_str,
                    error = %e,
                    "{}",
                    failure_log
                );
                Err(AppError::ExecutionBlocked(format!(
                    "Failed to start agent: {}",
                    e
                )))
            }
        }
    }

    /// Dual-channel emission of `task:execution_started` after a successful agent spawn.
    /// Non-fatal: logs warnings on failure rather than propagating errors.
    async fn emit_execution_started(&self, task_id_str: &str, project_id_str: &str) {
        let payload = serde_json::json!({
            "task_id": task_id_str,
            "project_id": project_id_str,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        if let Some(ref repo) = self.machine.context.services.external_events_repo {
            if let Err(e) = repo
                .insert_event(
                    "task:execution_started",
                    project_id_str,
                    &payload.to_string(),
                )
                .await
            {
                tracing::warn!(
                    task_id = task_id_str,
                    error = %e,
                    "Failed to persist task:execution_started event"
                );
            }
        }
        if let Some(ref publisher) = self.machine.context.services.webhook_publisher {
            publisher
                .publish(
                    crate::domain::entities::EventType::TaskExecutionStarted,
                    project_id_str,
                    payload,
                )
                .await;
        }
    }

    pub(super) async fn enter_executing_state(&self) -> AppResult<()> {
        let task_id_str = self.machine.context.task_id.as_str();
        let project_id_str = self.machine.context.project_id.as_str();

        self.check_plan_branch_active(task_id_str).await?;
        self.reset_stale_steps_on_entry(task_id_str).await;
        self.ensure_executing_branch_and_worktree(task_id_str, project_id_str)
            .await?;
        self.run_execution_freshness_check(task_id_str, project_id_str, "executing")
            .await?;
        self.run_and_store_pre_execution_setup(
            task_id_str,
            project_id_str,
            "execution",
            "execution_setup_log",
        )
        .await?;

        let prompt = self
            .build_execution_prompt(task_id_str, format!("Execute task: {}", task_id_str))
            .await;
        if !self
            .task_still_allows_execution_spawn(task_id_str, InternalStatus::Executing)
            .await
        {
            tracing::info!(
                task_id = task_id_str,
                "Skipping task_execution spawn because task status drifted during executing setup"
            );
            return Ok(());
        }
        tracing::debug!(
            task_id = task_id_str,
            prompt_len = prompt.len(),
            "Transition handler sending task_execution message"
        );
        let result = self
            .send_task_execution_message(
                task_id_str,
                &prompt,
                "Failed to send task execution message — agent not started",
            )
            .await;
        if result.is_ok() {
            self.emit_execution_started(task_id_str, project_id_str)
                .await;
        }
        result
    }

    pub(super) async fn enter_reexecuting_state(&self) -> AppResult<()> {
        let task_id_str = self.machine.context.task_id.as_str();
        let project_id_str = self.machine.context.project_id.as_str();

        self.check_plan_branch_active(task_id_str).await?;
        self.reset_stale_steps_on_entry(task_id_str).await;
        self.ensure_executing_branch_and_worktree(task_id_str, project_id_str)
            .await?;
        self.run_execution_freshness_check(task_id_str, project_id_str, "re_executing")
            .await?;
        self.run_and_store_pre_execution_setup(
            task_id_str,
            project_id_str,
            "execution",
            "execution_setup_log",
        )
        .await?;

        let prompt = self
            .build_execution_prompt(
                task_id_str,
                format!("Re-execute task (revision): {}", task_id_str),
            )
            .await;
        if !self
            .task_still_allows_execution_spawn(task_id_str, InternalStatus::ReExecuting)
            .await
        {
            tracing::info!(
                task_id = task_id_str,
                "Skipping task_execution spawn because task status drifted during re-executing setup"
            );
            return Ok(());
        }
        let result = self
            .send_task_execution_message(
                task_id_str,
                &prompt,
                "Failed to send re-execution message — agent not started",
            )
            .await;
        if result.is_ok() {
            self.emit_execution_started(task_id_str, project_id_str)
                .await;
        }
        result
    }
}
