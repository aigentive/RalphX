use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::{
    AutomationId, AutomationJudgeState, AutomationPlanJudgeState, AutomationRun, AutomationRunId,
    AutomationRunStatus, ChatConversationId,
};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutomationRunPublicationMetadata {
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub pr_title: Option<String>,
    pub pr_head_ref_name: Option<String>,
    pub pr_base_ref_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationJudgeTransitionGuard {
    Dispatch,
    Settle(DateTime<Utc>),
    /// Legacy escape for pre-token InProgress rows. Matches only NULL leases;
    /// switch to a dedicated judge_dispatch_id column iff lease renewal is ever added.
    LegacyNullLease,
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

    /// Find the latest automation run that owns the given conversation/workspace.
    ///
    /// Returns `None` when no automation run is linked to the conversation (for example an
    /// interactive, non-automation workspace). When multiple runs share a conversation id the
    /// implementation returns the one with the highest `run_index`.
    async fn find_run_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AutomationRun>>;

    async fn compare_and_swap_status(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        error_code: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<bool>;

    async fn compare_and_swap_status_with_agent_phase_started_at(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        agent_phase_started_at: DateTime<Utc>,
        error_code: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<bool>;

    async fn compare_and_swap_status_clearing_plan_pending_instructions(
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

    async fn clear_publication_metadata(
        &self,
        id: &AutomationRunId,
    ) -> AppResult<Option<AutomationRun>> {
        self.update_publication_metadata(id, AutomationRunPublicationMetadata::default())
            .await
    }

    /// Record PR merge metadata while the run is still waiting for a published PR signal.
    async fn update_merge_metadata(
        &self,
        id: &AutomationRunId,
        merge_commit_sha: Option<String>,
        pr_merged_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> AppResult<Option<AutomationRun>>;

    /// Atomically transition a published run to merged and record the merge facts in the same
    /// status-guarded write. Implementations should clear non-terminal error fields and reset
    /// signal check failures only when the status transition wins.
    async fn compare_and_swap_status_with_merge_metadata(
        &self,
        _id: &AutomationRunId,
        _from: AutomationRunStatus,
        _to: AutomationRunStatus,
        _merge_commit_sha: Option<String>,
        _pr_merged_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> AppResult<bool> {
        Err(AppError::Infrastructure(
            "automation run repository does not support atomic merge metadata transitions"
                .to_string(),
        ))
    }

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

    /// Update non-terminal warning/error fields for a published run without changing status.
    async fn update_published_run_error(
        &self,
        id: &AutomationRunId,
        error_code: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<Option<AutomationRun>>;

    async fn compare_and_swap_judge_state(
        &self,
        id: &AutomationRunId,
        from: AutomationJudgeState,
        to: AutomationJudgeState,
        guard: AutomationJudgeTransitionGuard,
        judge_verdict_json: Option<String>,
        judge_model_id: Option<String>,
        judge_lease_expires_at: Option<DateTime<Utc>>,
        error_detail: Option<String>,
    ) -> AppResult<bool>;

    /// Clear terminal judge state so a reopened run can be judged again.
    ///
    /// # Errors
    ///
    /// Returns an infrastructure error when the run cannot be updated.
    async fn clear_judge_state(&self, id: &AutomationRunId) -> AppResult<()>;

    async fn compare_and_swap_plan_judge_state(
        &self,
        id: &AutomationRunId,
        from: AutomationPlanJudgeState,
        to: AutomationPlanJudgeState,
        plan_judge_verdict_json: Option<String>,
        plan_judge_lease_expires_at: Option<DateTime<Utc>>,
    ) -> AppResult<bool>;

    /// Clears a stale verdict after a plan identity change while preserving the reset state.
    async fn clear_plan_judge_verdict(&self, id: &AutomationRunId) -> AppResult<bool> {
        let _ = id;
        Err(AppError::Infrastructure(
            "automation run repository does not support clearing a plan judge verdict".to_string(),
        ))
    }

    async fn clear_plan_judge_state(&self, id: &AutomationRunId) -> AppResult<bool>;

    async fn set_plan_pending_instructions(
        &self,
        id: &AutomationRunId,
        plan_pending_instructions: Option<String>,
    ) -> AppResult<Option<AutomationRun>>;

    async fn set_plan_revision_round(
        &self,
        id: &AutomationRunId,
        plan_revision_round: i64,
    ) -> AppResult<Option<AutomationRun>>;

    async fn set_plan_last_parked_artifact_id(
        &self,
        id: &AutomationRunId,
        plan_last_parked_artifact_id: Option<String>,
    ) -> AppResult<Option<AutomationRun>>;

    async fn set_plan_last_parked_artifact_ids(
        &self,
        id: &AutomationRunId,
        plan_last_parked_artifact_id: Option<String>,
        plan_last_parked_blueprint_artifact_id: Option<String>,
    ) -> AppResult<Option<AutomationRun>>;

    async fn set_plan_reminder_count(
        &self,
        id: &AutomationRunId,
        plan_reminder_count: i64,
    ) -> AppResult<Option<AutomationRun>>;

    async fn set_agent_phase_started_at(
        &self,
        id: &AutomationRunId,
        agent_phase_started_at: Option<DateTime<Utc>>,
    ) -> AppResult<Option<AutomationRun>>;

    /// Clear the terminal completion timestamp when a run is reopened.
    ///
    /// # Errors
    ///
    /// Returns an infrastructure error when the run cannot be updated.
    async fn clear_finished_at(&self, id: &AutomationRunId) -> AppResult<()>;

    /// Atomically insert the judge-created successor for the latest judged terminal run.
    /// Returns `None` when the previous run is stale, not `Done`, not signal-terminal, or the
    /// owning automation is no longer active.
    async fn create_judge_successor_run(
        &self,
        automation_id: &AutomationId,
        previous_run_id: &AutomationRunId,
        successor: AutomationRun,
    ) -> AppResult<Option<AutomationRun>>;

    /// Atomically mark the latest unjudged terminal run as skipped and insert its successor.
    /// Returns `None` when the previous run is stale, no longer unjudged, or no longer latest.
    async fn skip_judge_and_create_successor_run(
        &self,
        automation_id: &AutomationId,
        previous_run_id: &AutomationRunId,
        successor: AutomationRun,
    ) -> AppResult<Option<AutomationRun>>;

    /// Delete a run only when it is the automation's latest deletable run.
    ///
    /// Returns `1` when the row was deleted and `0` when it was missing, stale, or ineligible.
    ///
    /// # Errors
    ///
    /// Returns an infrastructure error when the repository cannot evaluate or delete the row.
    async fn delete_run_if_deletable(
        &self,
        automation_id: &AutomationId,
        run_id: &AutomationRunId,
    ) -> AppResult<usize>;

    async fn delete_for_automation(&self, automation_id: &AutomationId) -> AppResult<usize>;
}
