use async_trait::async_trait;

use crate::domain::entities::{
    TaskId, ValidationCommandResult, ValidationRun, ValidationRunStatus, ValidationRunWithResults,
};
use crate::error::AppResult;

#[async_trait]
pub trait ValidationRunRepository: Send + Sync {
    async fn create_run(&self, run: &ValidationRun) -> AppResult<()>;

    async fn update_run_status(
        &self,
        run_id: &str,
        status: ValidationRunStatus,
        completed_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> AppResult<()>;

    async fn add_command_result(&self, result: &ValidationCommandResult) -> AppResult<()>;

    async fn list_command_results_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Vec<ValidationCommandResult>>;

    async fn latest_run_with_results_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Option<ValidationRunWithResults>>;
}
