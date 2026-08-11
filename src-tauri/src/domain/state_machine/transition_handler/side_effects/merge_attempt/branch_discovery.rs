use super::*;
use crate::domain::state_machine::transition_handler::merge_helpers;

impl<'a> TransitionHandler<'a> {
    /// Log the result of branch discovery (orphaned task branch re-attach).
    pub(in crate::domain::state_machine::transition_handler::side_effects) async fn log_branch_discovery(
        &self,
        task: &mut Task,
        project: &crate::domain::entities::Project,
        task_repo: &Arc<dyn TaskRepository>,
        task_id_str: &str,
    ) {
        match merge_helpers::discover_and_attach_task_branch(task, project, task_repo).await {
            Ok(true) => {
                tracing::info!(
                    task_id = task_id_str,
                    branch = ?task.task_branch,
                    "Successfully recovered orphaned task branch"
                );
            }
            Ok(false) => {
                tracing::debug!(
                    task_id = task_id_str,
                    "No orphaned branch to recover (branch already set or doesn't exist)"
                );
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    task_id = task_id_str,
                    "Failed to discover orphaned task branch — continuing with existing flow"
                );
            }
        }
    }
}
