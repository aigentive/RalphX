use super::*;
use crate::domain::entities::{GitMode, InternalStatus};
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
                } else if project.git_mode == GitMode::Worktree {
                    return Err(AppError::ExecutionBlocked(format!(
                        "{}: task has no worktree_path before pre-execution setup",
                        GIT_ISOLATION_ERROR_PREFIX
                    )));
                } else {
                    tracing::warn!(
                        task_id = task_id_str,
                        "Skipping pre-execution setup: task has no worktree_path. \
                         Running install commands in the main repo is not safe."
                    );
                    return Ok(());
                };

                if !exec_cwd.exists() {
                    if project.git_mode == GitMode::Worktree {
                        return Err(AppError::ExecutionBlocked(format!(
                            "{}: task worktree_path '{}' does not exist before pre-execution setup",
                            GIT_ISOLATION_ERROR_PREFIX,
                            exec_cwd.display()
                        )));
                    }
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

    async fn persist_execution_task_update(
        &self,
        task_repo: &Arc<dyn TaskRepository>,
        task: &Task,
        task_id_str: &str,
        action: &'static str,
    ) -> AppResult<()> {
        if let Err(e) = task_repo.update(task).await {
            tracing::error!(
                task_id = task_id_str,
                error = %e,
                action,
                "Failed to persist execution task git state"
            );
            return Err(AppError::ExecutionBlocked(format!(
                "{}: failed to persist execution git state during {}: {}",
                GIT_ISOLATION_ERROR_PREFIX, action, e
            )));
        }
        Ok(())
    }

    async fn require_persisted_execution_worktree_ready(
        &self,
        task_id_str: &str,
        project: &Project,
    ) -> AppResult<()> {
        if project.git_mode != GitMode::Worktree {
            return Ok(());
        }

        let Some(ref task_repo) = self.machine.context.services.task_repo else {
            return Ok(());
        };
        let task_id = TaskId::from_string(task_id_str.to_string());
        let task = task_repo
            .get_by_id(&task_id)
            .await
            .map_err(|e| {
                AppError::ExecutionBlocked(format!(
                    "{}: failed to reload task before execution spawn: {}",
                    GIT_ISOLATION_ERROR_PREFIX, e
                ))
            })?
            .ok_or_else(|| {
                AppError::ExecutionBlocked(format!(
                    "{}: task disappeared before execution spawn",
                    GIT_ISOLATION_ERROR_PREFIX
                ))
            })?;

        validate_persisted_execution_worktree_path(&task, project, task_id_str)
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
            if let Ok(Some(project)) = project_repo.get_by_id(&project_id_typed).await {
                let repo_path = Path::new(&project.working_directory);
                for _ in 0..3 {
                    let Some(task) = task_repo.get_by_id(&task_id_typed).await? else {
                        return Err(AppError::TaskNotFound(task_id_str.to_string()));
                    };
                    let plan_branch = get_task_plan_branch(
                        &task,
                        &project,
                        &self.machine.context.services.plan_branch_repo,
                        &self.machine.context.services.task_repo,
                    )
                    .await;
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
                        self.machine.context.services.event_sink.as_deref(),
                        self.machine.context.services.activity_event_repo.as_ref(),
                        stage,
                        reconciliation_config(),
                    )
                    .await;
                    match apply_freshness_result(
                        freshness_result,
                        &task,
                        task_id_str,
                        task_repo,
                        self.machine.context.services.branch_update_repo.as_ref(),
                        self.machine
                            .context
                            .services
                            .branch_update_workflow
                            .as_ref(),
                        &project,
                        repo_path,
                        stage,
                    )
                    .await?
                    {
                        FreshnessApplyOutcome::Ready => return Ok(()),
                        FreshnessApplyOutcome::Updated(_) => continue,
                    }
                }
                return Err(AppError::ExecutionBlocked(
                    "Branch freshness checkpoints did not converge".to_string(),
                ));
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
                            self.persist_execution_task_update(
                                task_repo,
                                &task,
                                task_id_str,
                                "restore_task_worktree",
                            )
                            .await?;
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
                            self.persist_execution_task_update(
                                task_repo,
                                &task,
                                task_id_str,
                                "clear_stale_worktree_path",
                            )
                            .await?;
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
                            delete_existing_execution_worktree_or_block(
                                repo_path,
                                &stored,
                                task_id_str,
                                "deleted branch self-heal stored path cleanup",
                            )
                            .await?;
                        }
                        let expected_wt_path_str =
                            compute_task_worktree_path(&project, task_id_str);
                        let expected_wt_path = std::path::PathBuf::from(&expected_wt_path_str);
                        delete_existing_execution_worktree_or_block(
                            repo_path,
                            &expected_wt_path,
                            task_id_str,
                            "deleted branch self-heal expected path cleanup",
                        )
                        .await?;
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
                            Ok(new_worktree) => {
                                task.task_branch = Some(new_worktree.branch.clone());
                                task.task_branch_base_ref = Some(new_worktree.base_ref.clone());
                                task.task_branch_base_sha = Some(new_worktree.base_sha.clone());
                                task.worktree_path =
                                    Some(new_worktree.worktree_path.to_string_lossy().to_string());
                                task.merge_commit_sha = None;
                                task.touch();
                                tracing::info!(
                                    task_id = task_id_str,
                                    branch = %new_worktree.branch,
                                    base_ref = %new_worktree.base_ref,
                                    base_sha = %new_worktree.base_sha,
                                    worktree_path = %new_worktree.worktree_path.display(),
                                    "Self-healed: created fresh branch and worktree for deleted branch"
                                );
                                self.persist_execution_task_update(
                                    task_repo,
                                    &task,
                                    task_id_str,
                                    "persist_self_healed_branch",
                                )
                                .await?;
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
                            Ok(worktree) => {
                                tracing::info!(
                                    task_id = task_id_str,
                                    branch = %worktree.branch,
                                    base_ref = %worktree.base_ref,
                                    base_sha = %worktree.base_sha,
                                    worktree_path = %worktree.worktree_path.display(),
                                    "Created worktree with task branch"
                                );
                                task.task_branch = Some(worktree.branch);
                                task.task_branch_base_ref = Some(worktree.base_ref);
                                task.task_branch_base_sha = Some(worktree.base_sha);
                                task.worktree_path =
                                    Some(worktree.worktree_path.to_string_lossy().to_string());
                                task.touch();
                                self.persist_execution_task_update(
                                    task_repo,
                                    &task,
                                    task_id_str,
                                    "persist_new_task_branch",
                                )
                                .await?;
                            }
                            Err(e) => return Err(e),
                        }
                    }

                    if let Ok(Some(mut task)) = task_repo.get_by_id(&task_id).await {
                        if let Some(ref branch) = task.task_branch.clone() {
                            let expected_wt_path =
                                compute_task_worktree_path(&project, task_id_str);
                            let expected_wt_buf = std::path::PathBuf::from(&expected_wt_path);
                            if let Some(stored_wt) = task.worktree_path.as_deref() {
                                let stored_wt_buf = std::path::PathBuf::from(stored_wt);
                                if stored_wt_buf.exists() && stored_wt_buf != expected_wt_buf {
                                    tracing::warn!(
                                        task_id = task_id_str,
                                        stored_wt = %stored_wt_buf.display(),
                                        expected_wt = %expected_wt_buf.display(),
                                        "Task worktree_path points at a non-authoritative path — clearing for repair"
                                    );
                                    task.worktree_path = None;
                                    task.touch();
                                    self.persist_execution_task_update(
                                        task_repo,
                                        &task,
                                        task_id_str,
                                        "clear_wrong_worktree_path",
                                    )
                                    .await?;
                                }
                            }
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
                                persisted_task_branch_base_or_block(&task, branch, task_id_str)?;
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
                                        self.persist_execution_task_update(
                                            task_repo,
                                            &task,
                                            task_id_str,
                                            "persist_recreated_worktree_path",
                                        )
                                        .await?;
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
                                self.persist_execution_task_update(
                                    task_repo,
                                    &task,
                                    task_id_str,
                                    "persist_existing_worktree_path",
                                )
                                .await?;
                            }
                        }
                    }
                }

                self.require_persisted_execution_worktree_ready(task_id_str, &project)
                    .await?;
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
        task_state: &str,
        project_id_str: &str,
        failure_log: &str,
    ) -> AppResult<()> {
        match self
            .machine
            .context
            .services
            .chat_service
            .send_task_runtime_bootstrap_message(
                crate::domain::entities::ChatContextType::TaskExecution,
                task_id_str,
                prompt,
                task_state,
                project_id_str,
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
                "executing",
                project_id_str,
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
                "re_executing",
                project_id_str,
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

fn validate_persisted_execution_worktree_path(
    task: &Task,
    project: &Project,
    task_id_str: &str,
) -> AppResult<()> {
    if let Some(branch) = task.task_branch.as_deref() {
        persisted_task_branch_base_or_block(task, branch, task_id_str)?;
    }

    let worktree_path = task.worktree_path.as_deref().ok_or_else(|| {
        AppError::ExecutionBlocked(format!(
            "{}: task has no persisted worktree_path before execution spawn",
            GIT_ISOLATION_ERROR_PREFIX
        ))
    })?;
    let path = std::path::PathBuf::from(worktree_path);
    let project_path = std::path::PathBuf::from(&project.working_directory);

    if path == project_path {
        return Err(AppError::ExecutionBlocked(format!(
            "{}: task worktree_path points at the main project checkout",
            GIT_ISOLATION_ERROR_PREFIX
        )));
    }
    if is_merge_worktree_path(worktree_path) {
        return Err(AppError::ExecutionBlocked(format!(
            "{}: task worktree_path points at a merge worktree: {}",
            GIT_ISOLATION_ERROR_PREFIX, worktree_path
        )));
    }
    let expected_worktree_path =
        std::path::PathBuf::from(compute_task_worktree_path(project, task_id_str));
    if path != expected_worktree_path {
        return Err(AppError::ExecutionBlocked(format!(
            "{}: task worktree_path '{}' does not match expected execution worktree '{}'",
            GIT_ISOLATION_ERROR_PREFIX,
            worktree_path,
            expected_worktree_path.display()
        )));
    }
    if !path.exists() {
        return Err(AppError::ExecutionBlocked(format!(
            "{}: persisted task worktree_path '{}' does not exist before execution spawn",
            GIT_ISOLATION_ERROR_PREFIX, worktree_path
        )));
    }

    Ok(())
}

#[cfg(test)]
mod execution_worktree_validation_tests {
    use super::*;

    fn project_for_validation(root: &std::path::Path) -> Project {
        let mut project = Project::new(
            "validation-project".to_string(),
            root.join("main").to_string_lossy().to_string(),
        );
        project.git_mode = GitMode::Worktree;
        project.worktree_parent_directory = Some(root.to_string_lossy().to_string());
        project
    }

    fn task_for_validation(project: &Project, task_id_str: &str, path: Option<String>) -> Task {
        let mut task = Task::new(project.id.clone(), "validation task".to_string());
        task.id = TaskId::from_string(task_id_str.to_string());
        task.worktree_path = path;
        task
    }

    fn blocked_message(result: AppResult<()>) -> String {
        match result {
            Err(AppError::ExecutionBlocked(message)) => message,
            other => panic!("expected ExecutionBlocked, got {other:?}"),
        }
    }

    fn run_git(repo_path: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .output()
            .expect("git command should run");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn setup_git_repo(repo_path: &std::path::Path) {
        std::fs::create_dir_all(repo_path).expect("create repo dir");
        run_git(repo_path, &["init", "-b", "main"]);
        run_git(repo_path, &["config", "user.email", "test@test.com"]);
        run_git(repo_path, &["config", "user.name", "Test"]);
        std::fs::write(repo_path.join("README.md"), "# test repo").expect("write readme");
        run_git(repo_path, &["add", "."]);
        run_git(repo_path, &["commit", "-m", "initial commit"]);
    }

    #[test]
    fn validate_execution_worktree_rejects_missing_path_metadata() {
        let temp = tempfile::tempdir().expect("temp dir");
        let project = project_for_validation(temp.path());
        let task = task_for_validation(&project, "task-missing-metadata", None);

        let message = blocked_message(validate_persisted_execution_worktree_path(
            &task,
            &project,
            "task-missing-metadata",
        ));

        assert!(message.contains("no persisted worktree_path"));
    }

    #[test]
    fn validate_execution_worktree_rejects_main_checkout_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let project = project_for_validation(temp.path());
        let task = task_for_validation(
            &project,
            "task-main-checkout",
            Some(project.working_directory.clone()),
        );

        let message = blocked_message(validate_persisted_execution_worktree_path(
            &task,
            &project,
            "task-main-checkout",
        ));

        assert!(message.contains("main project checkout"));
    }

    #[test]
    fn validate_execution_worktree_rejects_merge_worktree_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let project = project_for_validation(temp.path());
        let task = task_for_validation(
            &project,
            "task-merge-path",
            Some(
                temp.path()
                    .join("merge-task-merge-path")
                    .to_string_lossy()
                    .to_string(),
            ),
        );

        let message = blocked_message(validate_persisted_execution_worktree_path(
            &task,
            &project,
            "task-merge-path",
        ));

        assert!(message.contains("merge worktree"));
    }

    #[test]
    fn validate_execution_worktree_rejects_non_authoritative_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let project = project_for_validation(temp.path());
        let wrong_path = temp.path().join("wrong-task-path");
        std::fs::create_dir_all(&wrong_path).expect("create wrong path");
        let task = task_for_validation(
            &project,
            "task-wrong-path",
            Some(wrong_path.to_string_lossy().to_string()),
        );

        let message = blocked_message(validate_persisted_execution_worktree_path(
            &task,
            &project,
            "task-wrong-path",
        ));

        assert!(message.contains("does not match expected execution worktree"));
    }

    #[test]
    fn validate_execution_worktree_rejects_missing_expected_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let project = project_for_validation(temp.path());
        let task_id_str = "task-missing-expected";
        let expected = compute_task_worktree_path(&project, task_id_str);
        let task = task_for_validation(&project, task_id_str, Some(expected));

        let message = blocked_message(validate_persisted_execution_worktree_path(
            &task,
            &project,
            task_id_str,
        ));

        assert!(message.contains("does not exist before execution spawn"));
    }

    #[test]
    fn validate_execution_worktree_accepts_existing_expected_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let project = project_for_validation(temp.path());
        let task_id_str = "task-valid-path";
        let expected = compute_task_worktree_path(&project, task_id_str);
        std::fs::create_dir_all(&expected).expect("create expected path");
        let task = task_for_validation(&project, task_id_str, Some(expected));

        validate_persisted_execution_worktree_path(&task, &project, task_id_str)
            .expect("existing expected execution worktree should validate");
    }

    #[test]
    fn stale_execution_worktree_cleanup_failure_blocks() {
        let temp = tempfile::tempdir().expect("temp dir");
        let worktree_path = temp.path().join("task-cleanup-blocked");

        let error = super::super::stale_execution_worktree_cleanup_blocked_error(
            "task-cleanup-blocked",
            &worktree_path,
            "delete denied",
            "test cleanup",
        );

        let AppError::ExecutionBlocked(message) = error else {
            panic!("cleanup failure should become ExecutionBlocked");
        };
        assert!(message.contains(GIT_ISOLATION_ERROR_PREFIX));
        assert!(message.contains("task-cleanup-blocked"));
        assert!(message.contains("delete denied"));
    }

    #[test]
    fn registered_task_worktree_reuse_requires_exact_path_and_branch() {
        let temp = tempfile::tempdir().expect("temp dir");
        let expected_path = temp.path().join("task-reusable");
        let expected_path_str = expected_path.to_string_lossy().to_string();
        let branch = "ralphx/validation-project/task-reusable";

        assert!(super::super::registered_task_worktree_matches_branch(
            &expected_path_str,
            Some(branch),
            &expected_path,
            branch,
        ));
        assert!(!super::super::registered_task_worktree_matches_branch(
            &expected_path_str,
            Some(branch),
            &expected_path,
            "ralphx/validation-project/task-other",
        ));
        assert!(!super::super::registered_task_worktree_matches_branch(
            &expected_path_str,
            Some(branch),
            &temp.path().join("task-other"),
            branch,
        ));
    }

    #[tokio::test]
    async fn delete_existing_execution_worktree_ignores_missing_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo_path = temp.path().join("not-a-repo");
        let worktree_path = temp.path().join("missing-task-worktree");

        super::super::delete_existing_execution_worktree_or_block(
            &repo_path,
            &worktree_path,
            "missing-task-worktree",
            "test missing cleanup",
        )
        .await
        .expect("missing worktree path should not block cleanup");
    }

    #[tokio::test]
    async fn existing_task_worktree_reuse_matches_registered_branch() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo_path = temp.path().join("repo");
        setup_git_repo(&repo_path);
        let worktree_path = temp.path().join("task-reusable");
        let worktree_path_str = worktree_path.to_string_lossy().to_string();
        let branch = "ralphx/validation-project/task-reusable";
        run_git(
            &repo_path,
            &["worktree", "add", "-b", branch, &worktree_path_str, "main"],
        );
        let registered_worktree_path = worktree_path
            .canonicalize()
            .expect("canonical worktree path");

        assert!(
            super::super::existing_task_worktree_is_reusable(
                &repo_path,
                &registered_worktree_path,
                branch,
                "task-reusable",
            )
            .await
        );
        assert!(
            !super::super::existing_task_worktree_is_reusable(
                &repo_path,
                &registered_worktree_path,
                "ralphx/validation-project/task-other",
                "task-reusable",
            )
            .await
        );
    }

    #[tokio::test]
    async fn delete_existing_execution_worktree_removes_registered_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo_path = temp.path().join("repo");
        setup_git_repo(&repo_path);
        let worktree_path = temp.path().join("task-delete");
        let worktree_path_str = worktree_path.to_string_lossy().to_string();
        let branch = "ralphx/validation-project/task-delete";
        run_git(
            &repo_path,
            &["worktree", "add", "-b", branch, &worktree_path_str, "main"],
        );

        super::super::delete_existing_execution_worktree_or_block(
            &repo_path,
            &worktree_path,
            "task-delete",
            "test registered cleanup",
        )
        .await
        .expect("registered worktree should be removed");

        assert!(!worktree_path.exists(), "worktree path should be removed");
        let worktrees = GitService::list_worktrees(&repo_path)
            .await
            .expect("list worktrees after cleanup");
        assert!(
            worktrees
                .iter()
                .all(|worktree| worktree.path != worktree_path_str),
            "deleted worktree should not remain registered"
        );
    }
}
