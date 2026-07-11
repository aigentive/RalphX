use super::*;
use crate::domain::state_machine::TransitionHandler;

impl<'a> TransitionHandler<'a> {
    pub(super) async fn enter_approved_state(&self) {
        self.machine
            .context
            .services
            .event_emitter
            .emit("task_completed", &self.machine.context.task_id)
            .await;
    }

    async fn persist_failed_task_metadata(&self, task_id: &str, data: &FailedData) {
        if let Some(ref task_repo) = self.machine.context.services.task_repo {
            let task_id_typed = TaskId::from_string(task_id.to_string());
            match task_repo.get_by_id(&task_id_typed).await {
                Ok(Some(task)) => {
                    let attempt_count = task
                        .metadata
                        .as_deref()
                        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                        .and_then(|v| v.get("auto_retry_count_executing").and_then(|c| c.as_u64()))
                        .unwrap_or(0) as u32;

                    let merged_metadata: String = if MetadataUpdate::key_exists_in(
                        "failure_error",
                        task.metadata.as_deref(),
                    ) {
                        tracing::debug!(
                            task_id = task_id,
                            attempt_count = attempt_count,
                            "failure_error already present (pre-computed); writing attempt_count only"
                        );
                        MetadataUpdate::new()
                            .with_u32("attempt_count", attempt_count)
                            .merge_into(task.metadata.as_deref())
                    } else {
                        let enriched_data = data.clone().with_attempt_count(attempt_count);
                        build_failed_metadata(&enriched_data).merge_into(task.metadata.as_deref())
                    };

                    let mut metadata_obj: serde_json::Map<String, serde_json::Value> =
                        serde_json::from_str(&merged_metadata).unwrap_or_default();

                    if ExecutionRecoveryMetadata::from_task_metadata(Some(&merged_metadata))
                        .unwrap_or(None)
                        .is_none()
                    {
                        let mut recovery = ExecutionRecoveryMetadata::new();
                        // Detect structural git errors (e.g., missing base branch detected by
                        // pre-validation in create_fresh_branch_and_worktree). These cannot be
                        // fixed by retrying — mark stop_retrying=true immediately.
                        if data.error.contains("structural:") {
                            recovery.stop_retrying = true;
                            recovery.unrecoverable_reason =
                                Some(StopRetryingReason::StructuralGitError);
                            recovery.append_event_with_state(
                                ExecutionRecoveryEvent::new(
                                    ExecutionRecoveryEventKind::Failed,
                                    ExecutionRecoverySource::System,
                                    ExecutionRecoveryReasonCode::StructuralGitError,
                                    &data.error,
                                )
                                .with_failure_source(ExecutionFailureSource::GitIsolation),
                                ExecutionRecoveryState::Failed,
                            );
                        } else {
                            recovery.append_event_with_state(
                                ExecutionRecoveryEvent::new(
                                    ExecutionRecoveryEventKind::Failed,
                                    ExecutionRecoverySource::System,
                                    ExecutionRecoveryReasonCode::Unknown,
                                    "Failed without pre-written recovery metadata (fallback)",
                                )
                                .with_failure_source(ExecutionFailureSource::Unknown),
                                ExecutionRecoveryState::Retrying,
                            );
                        }
                        if let Ok(recovery_value) = serde_json::to_value(&recovery) {
                            metadata_obj.insert("execution_recovery".to_string(), recovery_value);
                        }
                    }

                    if !metadata_obj.contains_key("failed_at") {
                        metadata_obj.insert(
                            "failed_at".to_string(),
                            serde_json::json!(Utc::now().to_rfc3339()),
                        );
                    }

                    let final_metadata =
                        serde_json::to_string(&serde_json::Value::Object(metadata_obj))
                            .unwrap_or(merged_metadata);

                    if let Err(e) = task_repo
                        .update_metadata(&task_id_typed, Some(final_metadata))
                        .await
                    {
                        tracing::error!(
                            task_id = task_id,
                            error = %e,
                            "Failed to update task failure metadata"
                        );
                    }
                }
                Ok(None) => {
                    tracing::error!(
                        task_id = task_id,
                        "Task not found when storing failure metadata"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        task_id = task_id,
                        error = %e,
                        "Error retrieving task for failure metadata"
                    );
                }
            }
        }
    }

    async fn fail_in_progress_steps_for_task(&self, task_id: &str) {
        if let Some(ref step_repo) = self.machine.context.services.step_repo {
            let task_id_typed = TaskId::from_string(task_id.to_string());
            match step_repo.get_by_task(&task_id_typed).await {
                Ok(steps) => {
                    for step in steps
                        .iter()
                        .filter(|s| s.status == TaskStepStatus::InProgress)
                    {
                        let mut failed_step = step.clone();
                        failed_step.status = TaskStepStatus::Failed;
                        failed_step.completion_note = Some("Task execution failed".to_string());
                        failed_step.completed_at = Some(Utc::now());

                        if let Err(e) = step_repo.update(&failed_step).await {
                            tracing::error!(
                                task_id = task_id,
                                step_id = %step.id,
                                error = %e,
                                "Failed to update in-progress step to failed status"
                            );
                        } else {
                            self.machine
                                .context
                                .services
                                .event_emitter
                                .emit("step:updated", &format!("{}", step.id))
                                .await;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        task_id = task_id,
                        error = %e,
                        "Failed to retrieve steps for failure handling"
                    );
                }
            }
        }
    }

    pub(super) async fn enter_failed_state(&self, data: &FailedData) {
        let task_id = self.machine.context.task_id.as_str();
        self.persist_failed_task_metadata(task_id, data).await;
        self.fail_in_progress_steps_for_task(task_id).await;
        self.machine
            .context
            .services
            .event_emitter
            .emit("task_failed", task_id)
            .await;
    }

    pub(super) async fn enter_merged_state(&self) {
        let task_id_str = &self.machine.context.task_id.clone();
        let task_id = TaskId::from_string(task_id_str.clone());
        let plan_branch_repo = &self.machine.context.services.plan_branch_repo;

        if let (Some(ref task_repo), Some(ref project_repo)) = (
            &self.machine.context.services.task_repo,
            &self.machine.context.services.project_repo,
        ) {
            if let Ok(Some(task)) = task_repo.get_by_id(&task_id).await {
                if task.category != TaskCategory::PlanMerge {
                    if let Ok(Some(project)) = project_repo
                        .get_by_id(&ProjectId::from_string(
                            self.machine.context.project_id.clone(),
                        ))
                        .await
                    {
                        let pr_sync_services =
                            super::super::merge_helpers::PlanBranchPrSyncServices::from_task_services(
                                &self.machine.context.services,
                            );
                        let _ = super::super::merge_helpers::sync_plan_branch_pr_after_regular_task_merge(
                            &task,
                            &project,
                            &pr_sync_services,
                        )
                        .await;
                    }
                }

                if task.category == TaskCategory::PlanMerge {
                    let project_id =
                        ProjectId::from_string(self.machine.context.project_id.clone());
                    let target_branch =
                        if let Ok(Some(project)) = project_repo.get_by_id(&project_id).await {
                            let repo_path = std::path::PathBuf::from(&project.working_directory);
                            self.post_merge_cleanup(
                                task_id_str,
                                &task_id,
                                &repo_path,
                                plan_branch_repo,
                            )
                            .await;
                            project.base_branch.clone().unwrap_or_default()
                        } else {
                            String::new()
                        };

                    // Emit plan:delivered if all session tasks are now Merged
                    if let Some(ref session_id) = task.ideation_session_id {
                        let session_id_str = session_id.as_str();
                        let project_id_str = self.machine.context.project_id.clone();

                        // Acquire per-session lock — clone Arc before awaiting to avoid
                        // holding a DashMap shard lock across an await point.
                        let lock_arc = {
                            let entry = self
                                .machine
                                .context
                                .services
                                .session_merge_locks
                                .entry(session_id_str.to_string())
                                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())));
                            Arc::clone(&*entry)
                        };
                        let _guard = lock_arc.lock().await;

                        // IDEMPOTENCY CHECK: default true (already delivered) on query failure
                        // so a failing query never causes a duplicate emission.
                        let already_delivered = if let Some(ref repo) =
                            self.machine.context.services.external_events_repo
                        {
                            repo.event_exists("plan:delivered", &project_id_str, session_id_str)
                                .await
                                .unwrap_or_else(|e| {
                                    tracing::warn!(
                                        session_id = session_id_str,
                                        error = %e,
                                        "plan:delivered idempotency check failed, assuming already delivered"
                                    );
                                    true
                                })
                        } else {
                            false // no repo → no idempotency check, proceed
                        };

                        if already_delivered {
                            tracing::debug!(
                                session_id = session_id_str,
                                "plan:delivered already emitted for session, skipping"
                            );
                        } else {
                            let all_merged = super::super::merge_helpers::check_session_all_merged(
                                session_id_str,
                                task_repo,
                            )
                            .await
                            .unwrap_or_else(|e| {
                                tracing::warn!(
                                    session_id = session_id_str,
                                    error = %e,
                                    "Failed to check session merge status for plan:delivered"
                                );
                                false
                            });

                            if all_merged {
                                let payload = serde_json::json!({
                                    "session_id": session_id_str,
                                    "task_id": task_id_str,
                                    "project_id": project_id_str,
                                    "commit_sha": task.merge_commit_sha,
                                    "target_branch": target_branch,
                                    "timestamp": Utc::now().to_rfc3339(),
                                });
                                if let Some(ref repo) =
                                    self.machine.context.services.external_events_repo
                                {
                                    let _ = repo
                                        .insert_event(
                                            "plan:delivered",
                                            &project_id_str,
                                            &payload.to_string(),
                                        )
                                        .await
                                        .map_err(|e| {
                                            tracing::warn!(
                                                session_id = session_id_str,
                                                error = %e,
                                                "Failed to persist plan:delivered event"
                                            )
                                        });
                                }
                                if let Some(ref publisher) =
                                    self.machine.context.services.webhook_publisher
                                {
                                    publisher
                                        .publish(
                                            ralphx_domain::entities::EventType::PlanDelivered,
                                            &project_id_str,
                                            payload,
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                }
            }
        }

        self.machine
            .context
            .services
            .dependency_manager
            .unblock_dependents(&self.machine.context.task_id)
            .await;

        if let Some(ref scheduler) = self.machine.context.services.task_scheduler {
            let scheduler = Arc::clone(scheduler);
            let merge_settle_ms = scheduler_config().merge_settle_ms;
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(merge_settle_ms)).await;
                scheduler.try_schedule_ready_tasks().await;
            });
        } else {
            tracing::warn!(
                task_id = self.machine.context.task_id.as_str(),
                "task_scheduler not wired — Ready tasks will not be auto-scheduled after Merged"
            );
        }

        if let Some(ref scheduler) = self.machine.context.services.task_scheduler {
            let scheduler = Arc::clone(scheduler);
            let project_id = self.machine.context.project_id.clone();
            tokio::spawn(async move {
                scheduler.try_retry_deferred_merges(&project_id).await;
            });
        }
    }
}

/// Record a reviewer agent spawn failure in task metadata.
///
/// Uses flat JSON fields: reviewer_spawn_failure_count, last_reviewer_spawn_error,
/// reviewer_spawn_failed_at. The reconciler reads reviewer_spawn_failure_count
/// to detect when the retry budget is exhausted and escalate.
pub(super) async fn record_reviewer_spawn_failure(
    task_repo: &Option<std::sync::Arc<dyn crate::domain::repositories::TaskRepository>>,
    task_id: &str,
    error: &str,
) {
    let Some(repo) = task_repo else { return };
    let tid = crate::domain::entities::TaskId::from_string(task_id.to_string());
    let Ok(Some(task)) = repo.get_by_id(&tid).await else {
        return;
    };

    let mut meta: serde_json::Value = task
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let current_count = meta
        .get("reviewer_spawn_failure_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let new_count = current_count + 1;

    meta["reviewer_spawn_failure_count"] = serde_json::json!(new_count);
    meta["last_reviewer_spawn_error"] = serde_json::json!(error);
    meta["reviewer_spawn_failed_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());

    let tid2 = crate::domain::entities::TaskId::from_string(task_id.to_string());
    if let Err(e) = repo.update_metadata(&tid2, Some(meta.to_string())).await {
        tracing::warn!(
            task_id = task_id,
            error = %e,
            "Failed to persist reviewer spawn failure metadata"
        );
    } else {
        tracing::warn!(
            task_id = task_id,
            count = new_count,
            "Recorded reviewer spawn failure ({}); reconciler will escalate when retry budget is exhausted",
            new_count,
        );
    }
}
