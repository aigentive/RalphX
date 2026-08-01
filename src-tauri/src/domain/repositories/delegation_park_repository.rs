use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::{
    AgentRunId, ChatConversationId, DelegationPark, DelegationParkId, DelegationParkState,
};
use crate::error::AppResult;

/// Repository contract for durable parent-turn delegation parks.
#[async_trait]
pub trait DelegationParkRepository: Send + Sync {
    /// Arm a new durable delegation park.
    ///
    /// # Errors
    ///
    /// Returns an error when the park cannot be persisted.
    async fn arm(&self, park: DelegationPark) -> AppResult<DelegationPark>;

    /// Load a delegation park by its ID.
    ///
    /// # Errors
    ///
    /// Returns an error when the park cannot be read.
    async fn get(&self, id: &DelegationParkId) -> AppResult<Option<DelegationPark>>;

    /// Load the armed park for a parent conversation, if one exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the lookup fails so callers do not mistake it for no armed park.
    async fn get_armed_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<DelegationPark>>;

    /// Return the newest park that still blocks settlement of this conversation's delegated job.
    ///
    /// Armed and waking parks always block. A woken park remains blocking until its resumed run
    /// exists; the settlement caller performs that run-authority check. `Ok(None)` means there is
    /// genuinely no blocking park.
    ///
    /// # Errors
    ///
    /// Returns an error when the lookup fails so callers cannot mistake it for settlement authority.
    async fn get_settlement_blocking_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<DelegationPark>>;

    /// List every currently armed delegation park.
    ///
    /// # Errors
    ///
    /// Returns an error when the parks cannot be read.
    async fn list_armed(&self) -> AppResult<Vec<DelegationPark>>;

    /// List armed parks that track a delegated agent run.
    ///
    /// # Errors
    ///
    /// Returns an error when the parks cannot be read.
    async fn list_armed_for_delegated_run(
        &self,
        run_id: &AgentRunId,
    ) -> AppResult<Vec<DelegationPark>>;

    /// Record the terminal status for one tracked delegated run.
    ///
    /// # Errors
    ///
    /// Returns an error when the park or tracked job cannot be updated.
    async fn record_job_settled(
        &self,
        id: &DelegationParkId,
        delegated_run_id: &AgentRunId,
        status: &str,
    ) -> AppResult<()>;

    /// CAS Armed -> Waking. Returns false if not currently Armed (supersession/expiry/other worker won).
    ///
    /// # Errors
    ///
    /// Returns an error when the compare-and-swap cannot be persisted.
    async fn claim_wake(&self, id: &DelegationParkId, expected_generation: i64) -> AppResult<bool>;

    /// Durably record a failed wake attempt and return the new attempt count.
    ///
    /// # Errors
    ///
    /// Returns an error when the failure or updated count cannot be persisted.
    async fn record_wake_failure(&self, id: &DelegationParkId, error: &str) -> AppResult<i32>;

    /// List parks whose wake claim has remained stalled since before `older_than`.
    ///
    /// # Errors
    ///
    /// Returns an error when stalled parks cannot be read.
    async fn list_wake_stalled(&self, older_than: DateTime<Utc>) -> AppResult<Vec<DelegationPark>>;

    /// Release a stale wake claim so startup recovery can safely re-dispatch it.
    ///
    /// # Errors
    ///
    /// Returns an error when the compare-and-swap cannot be persisted.
    async fn reset_wake_claim(&self, id: &DelegationParkId) -> AppResult<bool>;

    /// Settle a park into its terminal state, optionally recording an error.
    ///
    /// # Errors
    ///
    /// Returns an error when the park cannot be settled.
    async fn settle(
        &self,
        id: &DelegationParkId,
        state: DelegationParkState,
        error: Option<&str>,
    ) -> AppResult<()>;

    /// Supersede all armed or waking parks for a parent conversation.
    ///
    /// # Errors
    ///
    /// Returns an error when the parks cannot be superseded.
    async fn supersede_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<usize>;

    /// List armed parks whose deadlines are at or before `now`.
    ///
    /// # Errors
    ///
    /// Returns an error when expired parks cannot be read.
    async fn list_expired(&self, now: DateTime<Utc>) -> AppResult<Vec<DelegationPark>>;
}
