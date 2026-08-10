use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::RemoteConversationMessageRequest;
use crate::error::AppResult;

/// Durable store for spawn-free remote CONTINUATION intents.
///
/// Method names are deliberately DISTINCTIVE (`*_message_request`, never `*_request`): the
/// facade's detector (b) ties a registered command to a spawn-triggering surface by matching
/// write markers mechanically, and a name shared with the start table would make the two
/// surfaces indistinguishable in the audit.
#[async_trait]
pub trait RemoteConversationMessageRequestRepository: Send + Sync {
    async fn create_message_request(
        &self,
        request: RemoteConversationMessageRequest,
    ) -> AppResult<RemoteConversationMessageRequest>;

    async fn get_message_request(
        &self,
        id: &str,
    ) -> AppResult<Option<RemoteConversationMessageRequest>>;

    /// Atomic CAS: select ONE row with status=Pending (ORDER BY created_at ASC, id ASC), flip it
    /// to Dispatching stamping claimed_at + updated_at, return it. At-most-one claimant: a
    /// concurrent call gets None.
    async fn claim_pending_message_request(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteConversationMessageRequest>>;

    /// Dispatching -> Dispatched + agent_run_id + updated_at. Only applies while currently
    /// Dispatching (guard in WHERE), so a late writer cannot resurrect a swept row.
    async fn complete_message_request(
        &self,
        id: &str,
        agent_run_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()>;

    /// Dispatching -> Failed + error_code + updated_at. Only while currently Dispatching.
    async fn fail_message_request(
        &self,
        id: &str,
        error_code: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()>;

    /// Revoke-cancel: every Pending row for this device -> Cancelled + updated_at. Returns count.
    async fn cancel_pending_message_requests_for_device(
        &self,
        device_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64>;

    /// Startup sweep: Dispatching rows with claimed_at < claimed_before -> FailedStale. Returns
    /// count. Never re-queued: the outcome of a lost claim is unknown, and a re-dispatch would
    /// risk delivering the same turn twice.
    async fn fail_stale_dispatching_message_requests(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64>;
}
