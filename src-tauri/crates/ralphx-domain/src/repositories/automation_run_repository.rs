use async_trait::async_trait;

use crate::entities::{
    AutomationId, AutomationJudgeState, AutomationRun, AutomationRunId, AutomationRunStatus,
    ChatConversationId,
};
use crate::error::AppResult;

#[async_trait]
pub trait AutomationRunRepository: Send + Sync {
    async fn create_run(&self, run: AutomationRun) -> AppResult<AutomationRun>;

    async fn get_by_id(&self, id: &AutomationRunId) -> AppResult<Option<AutomationRun>>;

    async fn list_for_automation(
        &self,
        automation_id: &AutomationId,
    ) -> AppResult<Vec<AutomationRun>>;

    async fn latest_for_automation(
        &self,
        automation_id: &AutomationId,
    ) -> AppResult<Option<AutomationRun>>;

    async fn compare_and_swap_status(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        error_code: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<bool>;

    /// Attach the started conversation/workspace metadata while the run is still provisioning.
    /// Implementations return `None` when the run is missing or has already left provisioning.
    async fn update_start_metadata(
        &self,
        id: &AutomationRunId,
        conversation_id: &ChatConversationId,
        branch_name: Option<String>,
    ) -> AppResult<Option<AutomationRun>>;

    async fn compare_and_swap_judge_state(
        &self,
        id: &AutomationRunId,
        from: AutomationJudgeState,
        to: AutomationJudgeState,
        judge_verdict_json: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<bool>;

    async fn delete_for_automation(&self, automation_id: &AutomationId) -> AppResult<usize>;
}
