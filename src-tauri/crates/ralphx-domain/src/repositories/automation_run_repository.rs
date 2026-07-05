use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::{
    AutomationId, AutomationJudgeState, AutomationRun, AutomationRunId, AutomationRunStatus,
    ChatConversationId,
};
use crate::error::AppResult;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutomationRunPublicationMetadata {
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub pr_title: Option<String>,
    pub pr_head_ref_name: Option<String>,
    pub pr_base_ref_name: Option<String>,
}

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

    /// Record publication metadata observed from the owning agent workspace.
    /// Implementations return `None` when the run is missing or not in a running/published state.
    async fn update_publication_metadata(
        &self,
        id: &AutomationRunId,
        metadata: AutomationRunPublicationMetadata,
    ) -> AppResult<Option<AutomationRun>>;

    /// Record PR merge metadata while the run is still waiting for a published PR signal.
    async fn update_merge_metadata(
        &self,
        id: &AutomationRunId,
        merge_commit_sha: Option<String>,
        pr_merged_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> AppResult<Option<AutomationRun>>;

    /// Increment bounded scheduler-owned PR signal check failures for a published run.
    async fn increment_signal_check_failures(
        &self,
        id: &AutomationRunId,
    ) -> AppResult<Option<AutomationRun>>;

    /// Reset scheduler-owned PR signal check failures after a successful signal check.
    async fn reset_signal_check_failures(
        &self,
        id: &AutomationRunId,
    ) -> AppResult<Option<AutomationRun>>;

    async fn compare_and_swap_judge_state(
        &self,
        id: &AutomationRunId,
        from: AutomationJudgeState,
        to: AutomationJudgeState,
        judge_verdict_json: Option<String>,
        judge_model_id: Option<String>,
        judge_lease_expires_at: Option<DateTime<Utc>>,
        error_detail: Option<String>,
    ) -> AppResult<bool>;

    /// Atomically mark the latest unjudged terminal run as skipped and insert its successor.
    /// Returns `None` when the previous run is stale, no longer unjudged, or no longer latest.
    async fn skip_judge_and_create_successor_run(
        &self,
        automation_id: &AutomationId,
        previous_run_id: &AutomationRunId,
        successor: AutomationRun,
    ) -> AppResult<Option<AutomationRun>>;

    async fn delete_for_automation(&self, automation_id: &AutomationId) -> AppResult<usize>;
}
