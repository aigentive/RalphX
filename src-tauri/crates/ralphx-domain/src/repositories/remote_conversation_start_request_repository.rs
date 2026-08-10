use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::RemoteConversationStartRequest;
use crate::error::AppResult;

#[async_trait]
pub trait RemoteConversationStartRequestRepository: Send + Sync {
    async fn create_start_request(
        &self,
        request: RemoteConversationStartRequest,
    ) -> AppResult<RemoteConversationStartRequest>;

    async fn get_start_request(
        &self,
        id: &str,
    ) -> AppResult<Option<RemoteConversationStartRequest>>;

    /// Atomic CAS: select ONE row with status=Pending (ORDER BY created_at ASC, id ASC), flip it to
    /// Starting stamping claimed_at + updated_at, return it. At-most-one claimant: a concurrent call
    /// gets None. Use a transaction + `UPDATE ... WHERE id=? AND status='pending'` guarded by rows-affected.
    async fn claim_pending_start_request(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteConversationStartRequest>>;

    /// Starting -> Started + agent_run_id + updated_at. Only applies while currently Starting (guard in WHERE).
    async fn complete_start_request(
        &self,
        id: &str,
        agent_run_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()>;

    /// Starting -> Failed + error_code + updated_at. Only while currently Starting.
    async fn fail_start_request(
        &self,
        id: &str,
        error_code: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()>;

    /// Revoke-cancel: every Pending row for this device -> Cancelled + updated_at. Returns count changed.
    async fn cancel_pending_start_requests_for_device(
        &self,
        device_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64>;

    /// Startup sweep: Starting rows with claimed_at < claimed_before -> FailedStale + updated_at. Returns count.
    async fn fail_stale_starting_start_requests(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64>;
}
