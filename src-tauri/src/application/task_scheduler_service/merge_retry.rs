use super::*;

impl TaskSchedulerService {
    /// Re-trigger deferred merges for a project after a competing merge completes.
    ///
    /// Finds tasks in PendingMerge with `merge_deferred` metadata, clears the flag,
    /// and re-invokes their entry actions so `attempt_programmatic_merge()` runs again.
    pub(super) async fn retry_deferred_merges_impl(&self, project_id: &str) {
        use crate::domain::state_machine::transition_handler::{
            clear_merge_deferred_metadata, has_merge_deferred_metadata,
            is_merge_deferred_timed_out, set_trigger_origin, DEFERRED_MERGE_TIMEOUT_SECONDS,
        };

        let pid = ProjectId::from_string(project_id.to_string());
        let all_tasks = match self.task_repo.get_by_project(&pid).await {
            Ok(tasks) => tasks,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    project_id = project_id,
                    "Failed to fetch tasks for deferred merge retry"
                );
                return;
            }
        };

        // Count deferred tasks for logging
        let deferred_tasks: Vec<_> = all_tasks
            .iter()
            .filter(|t| {
                t.internal_status == InternalStatus::PendingMerge && has_merge_deferred_metadata(t)
            })
            .collect();

        let deferred_count = deferred_tasks.len();

        if deferred_count == 0 {
            tracing::debug!(project_id = project_id, "No deferred merges to retry");
            return;
        }

        tracing::info!(
            project_id = project_id,
            deferred_count = deferred_count,
            "Found deferred merges to retry (will retry one at a time)"
        );

        for task in deferred_tasks {
            // Extract metadata for logging
            let (target_branch, blocking_task_id) = task
                .metadata
                .as_ref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .map(|val| {
                    let target = val
                        .get("target_branch")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let blocker = val.get("blocking_task_id").and_then(|v| v.as_str());
                    (target.to_string(), blocker.map(|s| s.to_string()))
                })
                .unwrap_or_else(|| ("unknown".to_string(), None));

            // Warn if the merge has been deferred longer than the configured timeout.
            // This is a diagnostic indicator; the retry proceeds regardless (blocker just completed).
            if is_merge_deferred_timed_out(task) {
                tracing::warn!(
                    event = "deferred_merge_timeout_exceeded",
                    task_id = task.id.as_str(),
                    project_id = project_id,
                    target_branch = %target_branch,
                    timeout_seconds = DEFERRED_MERGE_TIMEOUT_SECONDS,
                    "Deferred merge exceeded timeout — retry was delayed beyond expected window"
                );
            }

            // Structured retry attempt event
            tracing::info!(
                event = "merge_retry_attempt",
                task_id = task.id.as_str(),
                project_id = project_id,
                target_branch = %target_branch,
                blocking_task_id = blocking_task_id.as_deref().unwrap_or("unknown"),
                remaining_deferred = deferred_count,
                "Re-triggering deferred merge attempt"
            );

            // Append auto_retry_triggered event before clearing deferred flag
            let mut updated = task.clone();

            // Get or create merge recovery metadata
            let mut recovery =
                MergeRecoveryMetadata::from_task_metadata(updated.metadata.as_deref())
                    .unwrap_or(None)
                    .unwrap_or_else(MergeRecoveryMetadata::new);

            // Count previous retry attempts from events
            let attempt_count = recovery
                .events
                .iter()
                .filter(|e| matches!(e.kind, MergeRecoveryEventKind::AutoRetryTriggered))
                .count() as u32
                + 1;

            // Create auto_retry_triggered event
            let auto_retry_event = MergeRecoveryEvent::new(
                MergeRecoveryEventKind::AutoRetryTriggered,
                MergeRecoverySource::Auto,
                MergeRecoveryReasonCode::TargetBranchBusy,
                format!(
                    "Automatic retry attempt {}: blocker task completed or exited merge workflow",
                    attempt_count
                ),
            )
            .with_target_branch(&target_branch)
            .with_attempt(attempt_count)
            .with_failure_source(MergeFailureSource::TransientGit);

            // Append event and update state to Retrying
            recovery.append_event_with_state(auto_retry_event, MergeRecoveryState::Retrying);

            // Update task metadata
            match recovery.update_task_metadata(updated.metadata.as_deref()) {
                Ok(updated_json) => {
                    updated.metadata = Some(updated_json);
                }
                Err(e) => {
                    tracing::error!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "Failed to serialize merge recovery metadata during retry"
                    );
                }
            }

            // Clear the legacy deferred flag
            clear_merge_deferred_metadata(&mut updated);
            set_trigger_origin(&mut updated, "recovery");
            updated.touch();

            if let Err(e) = self.task_repo.update(&updated).await {
                tracing::warn!(
                    event = "merge_retry_failed",
                    error = %e,
                    task_id = task.id.as_str(),
                    reason = "metadata_update_failed",
                    "Failed to update task metadata with retry event, skipping retry"
                );
                continue;
            }

            tracing::info!(
                task_id = task.id.as_str(),
                attempt = attempt_count,
                "Appended auto_retry_triggered event, re-invoking merge attempt"
            );

            // Re-invoke entry actions for PendingMerge to re-run attempt_programmatic_merge
            let transition_service = self.build_transition_service();
            transition_service
                .execute_entry_actions(&task.id, &updated, InternalStatus::PendingMerge)
                .await;

            // Only retry one deferred merge at a time to serialize them properly
            break;
        }
    }

    /// Retry main-branch merges that were deferred because agents were running.
    ///
    /// Called when the global running_count transitions to 0 (all agents idle).
    /// Finds tasks in PendingMerge with `main_merge_deferred` metadata, clears the flag,
    /// and re-invokes their entry actions to retry the main-branch merge.
    pub(super) async fn retry_main_merges_impl(&self) {
        use crate::domain::state_machine::transition_handler::{
            clear_main_merge_deferred_metadata, has_main_merge_deferred_metadata,
            is_main_merge_deferred_timed_out, set_trigger_origin, DEFERRED_MERGE_TIMEOUT_SECONDS,
        };

        let started_at = std::time::Instant::now();

        // Query all projects for main-merge-deferred tasks
        let projects_started_at = std::time::Instant::now();
        let projects = match self.project_repo.get_all().await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Failed to fetch projects for main merge retry"
                );
                return;
            }
        };
        let projects_elapsed_ms = projects_started_at.elapsed().as_millis();

        let mut deferred_tasks: Vec<Task> = Vec::new();
        let mut pending_merge_tasks_seen = 0usize;
        let task_scan_started_at = std::time::Instant::now();

        for project in &projects {
            let tasks = match self
                .task_repo
                .get_by_status(&project.id, InternalStatus::PendingMerge)
                .await
            {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        project_id = project.id.as_str(),
                        "Failed to fetch PendingMerge tasks for main merge retry"
                    );
                    continue;
                }
            };
            pending_merge_tasks_seen += tasks.len();

            for task in tasks {
                if has_main_merge_deferred_metadata(&task) {
                    deferred_tasks.push(task);
                }
            }
        }
        let task_scan_elapsed_ms = task_scan_started_at.elapsed().as_millis();

        let deferred_count = deferred_tasks.len();

        if deferred_count == 0 {
            tracing::info!(
                projects_seen = projects.len(),
                pending_merge_tasks_seen,
                deferred_count,
                projects_elapsed_ms,
                task_scan_elapsed_ms,
                elapsed_ms = started_at.elapsed().as_millis(),
                "Main merge retry scan completed with no deferred tasks"
            );
            return;
        }

        tracing::info!(
            deferred_count = deferred_count,
            "Found main-merge-deferred tasks to retry (all agents now idle)"
        );

        let mut skipped_sibling_guard = 0usize;
        let mut retry_attempted = false;
        for task in deferred_tasks {
            // Check if this deferred merge has exceeded the configured timeout.
            // If so, bypass the sibling guard and force a retry with a warning.
            let timed_out = is_main_merge_deferred_timed_out(&task);

            // Plan-level guard: skip retry if sibling tasks are not all terminal.
            // Bypassed when the deferred merge has exceeded DEFERRED_MERGE_TIMEOUT_SECONDS.
            if !timed_out {
                if let Some(ref session_id) = task.ideation_session_id {
                    match self.task_repo.get_by_ideation_session(session_id).await {
                        Ok(siblings) => {
                            let all_siblings_terminal = siblings.iter().all(|t| {
                                t.id == task.id
                                    || t.internal_status == InternalStatus::PendingMerge
                                    || t.is_terminal()
                            });
                            if !all_siblings_terminal {
                                skipped_sibling_guard += 1;
                                tracing::info!(
                                    task_id = task.id.as_str(),
                                    session_id = %session_id,
                                    "Skipping main merge retry: sibling plan tasks not yet terminal"
                                );
                                continue;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                task_id = task.id.as_str(),
                                "Failed to fetch siblings for plan-level merge guard, skipping retry"
                            );
                            continue;
                        }
                    }
                }
            } else {
                tracing::warn!(
                    event = "deferred_merge_timeout_forced_retry",
                    task_id = task.id.as_str(),
                    project_id = task.project_id.as_str(),
                    timeout_seconds = DEFERRED_MERGE_TIMEOUT_SECONDS,
                    "Deferred main merge has exceeded timeout — forcing retry regardless of sibling state"
                );
            }

            tracing::info!(
                event = "main_merge_retry_attempt",
                task_id = task.id.as_str(),
                project_id = task.project_id.as_str(),
                timed_out = timed_out,
                "Retrying deferred main merge (agents now idle)"
            );

            // Append main_merge_retry event before clearing deferred flag
            let mut updated = task.clone();

            // Get or create merge recovery metadata
            let mut recovery =
                MergeRecoveryMetadata::from_task_metadata(updated.metadata.as_deref())
                    .unwrap_or(None)
                    .unwrap_or_else(MergeRecoveryMetadata::new);

            // Count previous main merge retry attempts from events
            let attempt_count = recovery
                .events
                .iter()
                .filter(|e| matches!(e.kind, MergeRecoveryEventKind::MainMergeRetry))
                .count() as u32
                + 1;

            // Create main_merge_retry event
            // Extract target_branch from metadata if available
            let target_branch = updated
                .metadata
                .as_ref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|v| {
                    v.get("target_branch")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| "main".to_string());

            let (reason_code, retry_message) = if timed_out {
                (
                    MergeRecoveryReasonCode::DeferredTimeout,
                    format!(
                        "Main merge retry attempt {} (forced): deferred for >{}s, bypassing sibling guard",
                        attempt_count, DEFERRED_MERGE_TIMEOUT_SECONDS
                    ),
                )
            } else {
                (
                    MergeRecoveryReasonCode::AgentsRunning,
                    format!(
                        "Main merge retry attempt {}: all agents now idle",
                        attempt_count
                    ),
                )
            };

            let retry_event = MergeRecoveryEvent::new(
                MergeRecoveryEventKind::MainMergeRetry,
                MergeRecoverySource::Auto,
                reason_code,
                retry_message,
            )
            .with_target_branch(&target_branch)
            .with_attempt(attempt_count);

            // Append event and update state to Retrying
            recovery.append_event_with_state(retry_event, MergeRecoveryState::Retrying);

            // Update task metadata
            match recovery.update_task_metadata(updated.metadata.as_deref()) {
                Ok(updated_json) => {
                    updated.metadata = Some(updated_json);
                }
                Err(e) => {
                    tracing::error!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "Failed to serialize merge recovery metadata during main merge retry"
                    );
                }
            }

            // Clear the main_merge_deferred flag
            clear_main_merge_deferred_metadata(&mut updated);
            set_trigger_origin(&mut updated, "recovery");
            updated.touch();

            if let Err(e) = self.task_repo.update(&updated).await {
                tracing::warn!(
                    event = "main_merge_retry_failed",
                    error = %e,
                    task_id = task.id.as_str(),
                    reason = "metadata_update_failed",
                    "Failed to update task metadata, skipping main merge retry"
                );
                continue;
            }

            tracing::info!(
                task_id = task.id.as_str(),
                attempt = attempt_count,
                "Appended main_merge_retry event, re-invoking merge attempt"
            );

            // Re-invoke entry actions for PendingMerge to re-run attempt_programmatic_merge
            let transition_service = self.build_transition_service();
            transition_service
                .execute_entry_actions(&task.id, &updated, InternalStatus::PendingMerge)
                .await;
            retry_attempted = true;

            // Only retry one main merge at a time to serialize them properly
            break;
        }

        tracing::info!(
            projects_seen = projects.len(),
            pending_merge_tasks_seen,
            deferred_count,
            skipped_sibling_guard,
            retry_attempted,
            projects_elapsed_ms,
            task_scan_elapsed_ms,
            elapsed_ms = started_at.elapsed().as_millis(),
            "Main merge retry completed"
        );
    }
}
