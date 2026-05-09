use super::*;
use crate::domain::entities::ReviewScopeMetadata;
use crate::domain::review::{evaluate_merge_scope_backstop, MergeScopeBackstopViolation};

impl<'a> TransitionHandler<'a> {
    pub(super) async fn evaluate_merge_scope_backstop(
        &self,
        task: &Task,
        project: &Project,
        target_branch: &str,
    ) -> AppResult<Option<MergeScopeBackstopViolation>> {
        let Some(review_scope) =
            ReviewScopeMetadata::from_task_metadata(task.metadata.as_deref())
                .map_err(|e| crate::error::AppError::Validation(e.to_string()))?
        else {
            return Ok(None);
        };
        if review_scope.planned_paths.is_empty() {
            return Ok(None);
        }

        let repo_path = PathBuf::from(&project.working_directory);

        let Some(source_branch) = task.task_branch.as_deref() else {
            return Ok(None);
        };

        let diff = GitService::get_diff_stats_between(&repo_path, target_branch, source_branch).await?;
        Ok(evaluate_merge_scope_backstop(&review_scope, &diff.changed_files))
    }

    pub(super) async fn route_merge_scope_violation_to_revision(
        &self,
        task_id: &TaskId,
        task_id_str: &str,
        metadata: serde_json::Value,
    ) -> bool {
        let Some(transition_service) = &self.machine.context.services.transition_service else {
            tracing::warn!(
                task_id = task_id_str,
                "transition_service unavailable; cannot route merge scope guard back to revision"
            );
            return false;
        };

        match transition_service
            .reroute_merge_scope_drift_to_revision(task_id, metadata, true, "system")
            .await
        {
            Ok(_) => {
                tracing::info!(
                    task_id = task_id_str,
                    "Merge scope backstop routed task back to RevisionNeeded"
                );
                true
            }
            Err(e) => {
                tracing::warn!(
                    task_id = task_id_str,
                    error = %e,
                    "Failed to route merge scope guard back to RevisionNeeded"
                );
                false
            }
        }
    }
}
