use super::*;
use crate::domain::state_machine::services::ReviewStartResult;
use crate::domain::state_machine::services::TaskNotification;
use crate::domain::state_machine::TransitionHandler;

impl<'a> TransitionHandler<'a> {
    pub(super) async fn enter_pending_review_state(&self) {
        let review_result = self
            .machine
            .context
            .services
            .review_starter
            .start_ai_review(
                &self.machine.context.task_id,
                &self.machine.context.project_id,
            )
            .await;

        match &review_result {
            ReviewStartResult::Started { review_id } => {
                self.machine
                    .context
                    .services
                    .event_emitter
                    .emit_with_payload(
                        "review:update",
                        &self.machine.context.task_id,
                        &format!(r#"{{"type":"started","reviewId":"{}"}}"#, review_id),
                    )
                    .await;
            }
            ReviewStartResult::Disabled => {
                self.machine
                    .context
                    .services
                    .event_emitter
                    .emit_with_payload(
                        "review:update",
                        &self.machine.context.task_id,
                        r#"{"type":"disabled"}"#,
                    )
                    .await;
            }
            ReviewStartResult::Error(msg) => {
                if let Some(context) = self.machine.context.services.notification_context.clone() {
                    self.machine
                        .context
                        .services
                        .notifier
                        .notify(
                            context,
                            TaskNotification::ReviewError {
                                message: msg.clone(),
                            },
                        )
                        .await;
                }
            }
        }

        {
            let payload = serde_json::json!({
                "task_id": self.machine.context.task_id,
                "project_id": self.machine.context.project_id,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            if let Some(ref repo) = self.machine.context.services.external_events_repo {
                let _ = repo
                    .insert_event(
                        &ralphx_domain::entities::EventType::ReviewReady.to_string(),
                        &self.machine.context.project_id,
                        &payload.to_string(),
                    )
                    .await;
            }
            if let Some(ref publisher) = self.machine.context.services.webhook_publisher {
                publisher
                    .publish(
                        ralphx_domain::entities::EventType::ReviewReady,
                        &self.machine.context.project_id,
                        payload,
                    )
                    .await;
            }
        }
    }

    async fn run_reviewing_freshness_check(
        &self,
        task_id_str: &str,
        project_id_str: &str,
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
                        "reviewing",
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
                        "reviewing",
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

    async fn ensure_review_worktree_ready(&self, task_id_str: &str) -> AppResult<()> {
        if let (Some(ref task_repo), Some(ref project_repo)) = (
            &self.machine.context.services.task_repo,
            &self.machine.context.services.project_repo,
        ) {
            let task_id_typed = TaskId::from_string(task_id_str.to_string());
            if let Ok(Some(mut task)) = task_repo.get_by_id(&task_id_typed).await {
                if task
                    .worktree_path
                    .as_deref()
                    .map(is_merge_worktree_path)
                    .unwrap_or(false)
                {
                    match project_repo.get_by_id(&task.project_id).await {
                        Ok(Some(project)) => {
                            let repo_path = Path::new(&project.working_directory);
                            match restore_task_worktree(&mut task, &project, repo_path).await {
                                Ok(restored) => {
                                    tracing::info!(
                                        task_id = task_id_str,
                                        restored_path = %restored.display(),
                                        "L2: restored merge-prefixed worktree_path before reviewer spawn"
                                    );
                                    if let Err(e) = task_repo.update(&task).await {
                                        tracing::warn!(
                                            task_id = task_id_str,
                                            error = %e,
                                            "L2: failed to persist restored worktree_path"
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        task_id = task_id_str,
                                        error = %e,
                                        "L2: failed to restore task worktree in Reviewing entry — will fail as ReviewWorktreeMissing"
                                    );
                                }
                            }
                        }
                        Ok(None) => {
                            tracing::warn!(
                                task_id = task_id_str,
                                "L2: project not found for worktree restoration"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                task_id = task_id_str,
                                error = %e,
                                "L2: failed to fetch project for worktree restoration"
                            );
                        }
                    }
                }

                if let Some(ref wt_path_str) = task.worktree_path {
                    let wt_path = std::path::Path::new(wt_path_str);
                    if wt_path.exists() {
                        match crate::application::git_service::GitService::has_conflict_markers(
                            wt_path,
                        )
                        .await
                        {
                            Ok(true) => {
                                tracing::warn!(
                                    task_id = task_id_str,
                                    worktree = %wt_path.display(),
                                    "Conflict markers detected without directional freshness evidence — routing to review-worktree attention"
                                );
                                let mut task_metadata: serde_json::Value = task
                                    .metadata
                                    .as_deref()
                                    .and_then(|s| serde_json::from_str(s).ok())
                                    .unwrap_or_else(|| serde_json::json!({}));
                                task_metadata["conflict_markers_detected"] =
                                    serde_json::json!(true);
                                if let Err(e) = task_repo
                                    .update_metadata(
                                        &task_id_typed,
                                        Some(task_metadata.to_string()),
                                    )
                                    .await
                                {
                                    tracing::warn!(
                                        task_id = task_id_str,
                                        error = %e,
                                        "Failed to persist conflict marker metadata"
                                    );
                                }
                                return Err(AppError::ReviewWorktreeConflictMarkers);
                            }
                            Ok(false) => {
                                tracing::debug!(
                                    task_id = task_id_str,
                                    worktree = %wt_path.display(),
                                    "Conflict marker scan passed — worktree is clean"
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    task_id = task_id_str,
                                    error = %e,
                                    "Conflict marker scan failed — proceeding with review anyway"
                                );
                            }
                        }
                    } else {
                        tracing::error!(
                            task_id = task_id_str,
                            worktree = %wt_path.display(),
                            "Reviewer spawn blocked: worktree directory does not exist"
                        );
                        let mut task_meta: serde_json::Value = task
                            .metadata
                            .as_deref()
                            .and_then(|s| serde_json::from_str(s).ok())
                            .unwrap_or_else(|| serde_json::json!({}));
                        task_meta["worktree_missing_at_review"] = serde_json::json!(true);
                        if let Err(me) = task_repo
                            .update_metadata(&task_id_typed, Some(task_meta.to_string()))
                            .await
                        {
                            tracing::warn!(
                                task_id = task_id_str,
                                error = %me,
                                "Failed to persist worktree_missing_at_review metadata"
                            );
                        }
                        return Err(crate::error::AppError::ReviewWorktreeMissing);
                    }
                } else {
                    tracing::error!(
                        task_id = task_id_str,
                        "Reviewer spawn blocked: task has no worktree_path set"
                    );
                    return Err(crate::error::AppError::ReviewWorktreeMissing);
                }
            }
        }

        Ok(())
    }

    async fn spawn_reviewer_agent(&self, task_id: &str) {
        let prompt = format!("Review task: {}", task_id);
        let project_id = self.machine.context.project_id.as_str();

        tracing::info!(
            task_id = task_id,
            "on_enter(Reviewing): Spawning reviewer agent via ChatService"
        );

        let result = self
            .machine
            .context
            .services
            .chat_service
            .send_task_runtime_bootstrap_message(
                crate::domain::entities::ChatContextType::Review,
                task_id,
                &prompt,
                "reviewing",
                project_id,
            )
            .await;

        match result {
            Ok(result) if result.was_queued => {
                tracing::info!(
                    task_id = task_id,
                    "Agent already running for this task — treating on_enter(Reviewing) as no-op"
                );
            }
            Ok(_) => {
                tracing::info!(task_id = task_id, "Reviewer agent spawned successfully");
            }
            Err(e) => {
                tracing::error!(task_id = task_id, error = %e, "Failed to spawn reviewer agent");
                super::outcomes::record_reviewer_spawn_failure(
                    &self.machine.context.services.task_repo,
                    task_id,
                    &e.to_string(),
                )
                .await;
            }
        }
    }

    pub(super) async fn enter_reviewing_state(&self) -> AppResult<()> {
        let task_id_str = self.machine.context.task_id.as_str();
        let project_id_str = self.machine.context.project_id.as_str();

        self.run_reviewing_freshness_check(task_id_str, project_id_str)
            .await?;
        self.ensure_review_worktree_ready(task_id_str).await?;
        self.run_and_store_pre_execution_setup(
            task_id_str,
            project_id_str,
            "review",
            "review_setup_log",
        )
        .await?;
        self.spawn_reviewer_agent(task_id_str).await;

        Ok(())
    }

    pub(super) async fn enter_review_passed_state(&self) {
        if let Some(task_repo) = &self.machine.context.services.task_repo {
            let task_id_typed = TaskId::from_string(self.machine.context.task_id.clone());
            if let Ok(Some(task)) = task_repo.get_by_id(&task_id_typed).await {
                let mut meta: serde_json::Value = task
                    .metadata
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                freshness::FreshnessMetadata::cleanup(
                    freshness::FreshnessCleanupScope::RoutingOnly,
                    &mut meta,
                );
                if let Err(e) = task_repo
                    .update_metadata(&task_id_typed, Some(meta.to_string()))
                    .await
                {
                    tracing::warn!(
                        task_id = %self.machine.context.task_id,
                        error = %e,
                        "Failed to clear freshness routing metadata on ReviewPassed"
                    );
                }
            }
        }

        self.machine
            .context
            .services
            .event_emitter
            .emit("review:ai_approved", &self.machine.context.task_id)
            .await;

        {
            let payload = serde_json::json!({
                "task_id": self.machine.context.task_id,
                "project_id": self.machine.context.project_id,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            if let Some(ref repo) = self.machine.context.services.external_events_repo {
                let _ = repo
                    .insert_event(
                        &ralphx_domain::entities::EventType::ReviewApproved.to_string(),
                        &self.machine.context.project_id,
                        &payload.to_string(),
                    )
                    .await;
            }
            if let Some(ref publisher) = self.machine.context.services.webhook_publisher {
                publisher
                    .publish(
                        ralphx_domain::entities::EventType::ReviewApproved,
                        &self.machine.context.project_id,
                        payload,
                    )
                    .await;
            }
        }
    }

    pub(super) async fn enter_escalated_state(&self) {
        self.machine
            .context
            .services
            .event_emitter
            .emit("review:escalated", &self.machine.context.task_id)
            .await;

        {
            let payload = serde_json::json!({
                "task_id": self.machine.context.task_id,
                "project_id": self.machine.context.project_id,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            if let Some(ref repo) = self.machine.context.services.external_events_repo {
                let _ = repo
                    .insert_event(
                        &ralphx_domain::entities::EventType::ReviewEscalated.to_string(),
                        &self.machine.context.project_id,
                        &payload.to_string(),
                    )
                    .await;
            }
            if let Some(ref publisher) = self.machine.context.services.webhook_publisher {
                publisher
                    .publish(
                        ralphx_domain::entities::EventType::ReviewEscalated,
                        &self.machine.context.project_id,
                        payload,
                    )
                    .await;
            }
        }
    }

    pub(super) async fn enter_revision_needed_state(&self) {
        {
            let payload = serde_json::json!({
                "task_id": self.machine.context.task_id,
                "project_id": self.machine.context.project_id,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            if let Some(ref repo) = self.machine.context.services.external_events_repo {
                let _ = repo
                    .insert_event(
                        &ralphx_domain::entities::EventType::ReviewChangesRequested.to_string(),
                        &self.machine.context.project_id,
                        &payload.to_string(),
                    )
                    .await;
            }
            if let Some(ref publisher) = self.machine.context.services.webhook_publisher {
                publisher
                    .publish(
                        ralphx_domain::entities::EventType::ReviewChangesRequested,
                        &self.machine.context.project_id,
                        payload,
                    )
                    .await;
            }
        }
    }
}
