use crate::error::{AppError, AppResult};

impl<'a> super::super::TransitionHandler<'a> {
    pub(super) async fn enter_branch_update_state(&self) -> AppResult<()> {
        let task_id = &self.machine.context.task_id;
        let prompt = format!(
            "Resolve the active branch update for task {task_id}. Start with get_branch_update_context and signal completion or a dedicated branch-update failure."
        );
        let repository = self
            .machine
            .context
            .services
            .branch_update_repo
            .as_ref()
            .ok_or_else(|| {
                AppError::ExecutionBlocked(
                    "Branch update authority repository is unavailable".to_string(),
                )
            })?;
        let operation = repository
            .get_active_operation(&crate::domain::entities::TaskId::from_string(
                task_id.to_string(),
            ))
            .await?
            .ok_or_else(|| {
                AppError::ExecutionBlocked(
                    "Branch update state has no active durable operation".to_string(),
                )
            })?;
        let workspace = operation.workspace_path.ok_or_else(|| {
            AppError::ExecutionBlocked(
                "Branch update operation has no isolated workspace".to_string(),
            )
        })?;
        let workflow = self
            .machine
            .context
            .services
            .branch_update_workflow
            .as_ref()
            .ok_or_else(|| {
                AppError::ExecutionBlocked(
                    "Branch update workflow adapter is unavailable".to_string(),
                )
            })?;
        workflow
            .start_resolver(task_id, &prompt, &workspace)
            .await
            .map_err(|error| AppError::ExecutionBlocked(error.to_string()))?;
        Ok(())
    }
}
