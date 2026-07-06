use super::*;

impl<R: Runtime> TaskSchedulerService<R> {
    pub(super) async fn find_oldest_retryable_pending_review_task(&self) -> Option<Task> {
        let active_project = self.active_project_id.read().await.clone();
        let projects = if let Some(project_id) = active_project {
            match self.project_repo.get_by_id(&project_id).await {
                Ok(Some(project)) => vec![project],
                Ok(None) => return None,
                Err(error) => {
                    tracing::warn!(
                        project_id = project_id.as_str(),
                        error = %error,
                        "Failed to load active project while scanning retryable PendingReview tasks"
                    );
                    return None;
                }
            }
        } else {
            match self.project_repo.get_all().await {
                Ok(projects) => projects,
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Failed to load projects while scanning retryable PendingReview tasks"
                    );
                    return None;
                }
            }
        };

        let mut candidates = Vec::new();
        let now = Utc::now();

        for project in projects {
            let tasks = match self
                .task_repo
                .get_by_status(&project.id, InternalStatus::PendingReview)
                .await
            {
                Ok(tasks) => tasks,
                Err(error) => {
                    tracing::warn!(
                        project_id = project.id.as_str(),
                        error = %error,
                        "Failed to load PendingReview tasks while scanning retryable review tasks"
                    );
                    continue;
                }
            };

            for task in tasks {
                let Some(metadata_str) = task.metadata.as_deref() else {
                    continue;
                };
                let Ok(metadata_val) = serde_json::from_str::<serde_json::Value>(metadata_str) else {
                    continue;
                };
                let freshness = FreshnessMetadata::from_task_metadata(&metadata_val);
                let Some(backoff_until) = freshness.freshness_backoff_until else {
                    continue;
                };
                if freshness.freshness_origin_state.as_deref() != Some("reviewing")
                    || now < backoff_until
                {
                    continue;
                }
                if !self.project_has_execution_capacity(&task.project_id).await {
                    tracing::debug!(
                        task_id = task.id.as_str(),
                        project_id = task.project_id.as_str(),
                        "Skipping retryable PendingReview task: project execution capacity reached"
                    );
                    continue;
                }
                candidates.push(task);
            }
        }

        candidates.sort_by(|a, b| {
            a.updated_at
                .cmp(&b.updated_at)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        candidates.into_iter().next()
    }

    pub(super) async fn retry_pending_review_task(&self, task: &Task) {
        tracing::info!(
            task_id = task.id.as_str(),
            task_title = task.title.as_str(),
            "Retrying PendingReview task after freshness backoff expiry"
        );

        if !self.execution_state.try_start_scheduling(task.id.as_str()) {
            tracing::debug!(
                task_id = task.id.as_str(),
                "Scheduler: PendingReview task already being retried by another caller, skipping"
            );
            return;
        }

        let transition_service = self.build_transition_service();
        transition_service
            .execute_entry_actions(&task.id, task, InternalStatus::PendingReview)
            .await;

        self.execution_state.finish_scheduling(task.id.as_str());
    }

    pub(super) async fn count_active_slot_consuming_contexts_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Option<u32> {
        let registry_entries = self.running_agent_registry.list_all().await;
        let mut count = 0u32;

        for (key, info) in registry_entries {
            if info.pid == 0 {
                continue;
            }

            if key.context_type == "ideation" || key.context_type == "session" {
                let session_id = IdeationSessionId::from_string(key.context_id.clone());
                let session = match self.ideation_session_repo.get_by_id(&session_id).await {
                    Ok(Some(session)) => session,
                    Ok(None) => continue,
                    Err(error) => {
                        tracing::warn!(
                            project_id = project_id.as_str(),
                            error = %error,
                            "Failed to load ideation session while checking project capacity"
                        );
                        return None;
                    }
                };

                if session.project_id != *project_id {
                    continue;
                }

                let slot_key = format!("{}/{}", key.context_type, key.context_id);
                if self.execution_state.is_interactive_idle(&slot_key) {
                    continue;
                }

                count += 1;
                continue;
            }

            let context_type = match key.context_type.parse::<ChatContextType>() {
                Ok(value) => value,
                Err(_) => continue,
            };

            if !uses_execution_slot(context_type) {
                continue;
            }

            let task_id = crate::domain::entities::TaskId::from_string(key.context_id.clone());
            let task = match self.task_repo.get_by_id(&task_id).await {
                Ok(Some(task)) => task,
                Ok(None) => continue,
                Err(error) => {
                    tracing::warn!(
                        project_id = project_id.as_str(),
                        error = %error,
                        "Failed to load task while checking project capacity"
                    );
                    return None;
                }
            };

            if task.project_id != *project_id
                || !context_matches_running_status_for_gc(context_type, task.internal_status)
            {
                continue;
            }

            count += 1;
        }

        Some(count)
    }

    pub(super) async fn project_has_execution_capacity(&self, project_id: &ProjectId) -> bool {
        let Some(repo) = self.execution_settings_repo.as_ref() else {
            return true;
        };

        let settings = match repo.get_settings(Some(project_id)).await {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(
                    project_id = project_id.as_str(),
                    error = %error,
                    "Failed to load execution settings while checking project capacity"
                );
                return true;
            }
        };

        let Some(running_project_total) = self
            .count_active_slot_consuming_contexts_for_project(project_id)
            .await
        else {
            return true;
        };

        self.execution_state
            .can_start_execution_context(running_project_total, settings.max_concurrent_tasks)
    }

    /// Check if a task's plan branch is no longer Active (Merged or Abandoned).
    /// Returns true if the task should NOT be scheduled. Fail-open on errors.
    /// Uses `execution_plan_id` (not `session_id`) to handle re-accept flows where
    /// multiple PlanBranch records exist for the same session.
    pub(super) async fn is_plan_branch_inactive(&self, task: &Task) -> bool {
        let exec_plan_id = match &task.execution_plan_id {
            Some(id) => id,
            None => return false, // Non-plan tasks are always schedulable
        };
        let plan_branch_repo = match &self.plan_branch_repo {
            Some(repo) => repo,
            None => return false, // No repo available, fail-open
        };
        match plan_branch_repo.get_by_execution_plan_id(exec_plan_id).await {
            Ok(Some(branch)) => {
                use crate::domain::entities::PlanBranchStatus;
                !matches!(branch.status, PlanBranchStatus::Active)
            }
            Ok(None) => false, // No branch found, fail-open
            Err(_) => false,   // Error, fail-open
        }
    }

    /// Check if a task's execution plan is no longer Active.
    /// Returns true if the task should NOT be scheduled.
    pub(super) async fn is_execution_plan_inactive(&self, task: &Task) -> bool {
        let exec_plan_id = match &task.execution_plan_id {
            Some(id) => id,
            None => return false,
        };
        let execution_plan_repo = match &self.execution_plan_repo {
            Some(repo) => repo,
            None => return false,
        };

        match execution_plan_repo.get_by_id(exec_plan_id).await {
            Ok(Some(plan)) => !matches!(
                plan.status,
                crate::domain::entities::ExecutionPlanStatus::Active
            ),
            Ok(None) => false,
            Err(_) => false,
        }
    }

    /// Check if a task has any blocker whose status is not dependency-satisfied.
    /// Returns true if the task should NOT be scheduled. Fail-open on errors.
    pub(super) async fn has_unsatisfied_dependencies(&self, task: &Task) -> bool {
        let blocker_ids = match self.task_dependency_repo.get_blockers(&task.id).await {
            Ok(ids) => ids,
            Err(_) => return false,
        };
        if blocker_ids.is_empty() {
            return false;
        }
        for blocker_id in &blocker_ids {
            match self.task_repo.get_by_id(blocker_id).await {
                Ok(Some(blocker)) => {
                    if blocker.internal_status.is_active_dependency_blocker() {
                        return true;
                    }
                }
                Ok(None) => {} // deleted blocker = satisfied
                Err(_) => {}   // fail-open
            }
        }
        false
    }

    /// Re-block a Ready task that has unsatisfied dependencies.
    /// Sets status to Blocked with a descriptive reason listing unsatisfied blocker titles.
    pub(super) async fn reblock_task(&self, task: &Task) {
        let blocker_ids = self
            .task_dependency_repo
            .get_blockers(&task.id)
            .await
            .unwrap_or_default();

        let mut reasons = Vec::new();
        for bid in &blocker_ids {
            if let Ok(Some(b)) = self.task_repo.get_by_id(bid).await {
                if b.internal_status.is_active_dependency_blocker() {
                    let label = if b.internal_status == InternalStatus::Failed {
                        format!("\"{}\" (failed)", b.title)
                    } else {
                        format!("\"{}\" ({})", b.title, b.internal_status)
                    };
                    reasons.push(label);
                }
            }
        }

        let mut updated = task.clone();
        updated.internal_status = InternalStatus::Blocked;
        updated.blocked_reason = if reasons.is_empty() {
            Some("Dependency check failed".to_string())
        } else {
            Some(format!("Waiting for: {}", reasons.join(", ")))
        };
        updated.touch();

        // Use optimistic lock — if task already moved out of Ready, this is a no-op
        match self
            .task_repo
            .update_with_expected_status(&updated, InternalStatus::Ready)
            .await
        {
            Ok(true) => {
                tracing::warn!(
                    task_id = task.id.as_str(),
                    "Scheduler: re-blocked Ready task with unsatisfied dependencies"
                );
                let _ = self
                    .task_repo
                    .persist_status_change(
                        &task.id,
                        InternalStatus::Ready,
                        InternalStatus::Blocked,
                        "scheduler_dep_gate",
                    )
                    .await;
            }
            Ok(false) => {
                tracing::debug!(
                    task_id = task.id.as_str(),
                    "Scheduler: task already moved from Ready, skipping re-block"
                );
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    task_id = task.id.as_str(),
                    "Failed to re-block task with unsatisfied dependencies"
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod helpers_tests;
