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

    async fn record_validated_content_fingerprint(
        &self,
        run_id: &str,
        fingerprint: Option<String>,
    ) -> AppResult<()>;

    async fn promote_run_to_commit(&self, run_id: &str, commit_sha: &str) -> AppResult<()>;

    async fn mark_running_runs_error(
        &self,
        completed_at: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<u64>;

    async fn add_command_result(&self, result: &ValidationCommandResult) -> AppResult<()>;

    async fn list_command_results_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Vec<ValidationCommandResult>>;

    async fn latest_run_with_results_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Option<ValidationRunWithResults>>;

    async fn latest_non_baseline_run_with_results_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Option<ValidationRunWithResults>>;
}
