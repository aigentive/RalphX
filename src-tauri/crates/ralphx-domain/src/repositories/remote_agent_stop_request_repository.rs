use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::{ChatConversationId, RemoteAgentStopRequest};
use crate::error::AppResult;

/// Durable store for remote STOP intents.
///
/// Method names are deliberately DISTINCTIVE (`create_stop_request`,
/// `claim_pending_stop_request`, …) rather than the generic `create`/`update` used elsewhere:
/// the remote authority audit's detector (b) matches call-graph tokens against write-site
/// markers, and a generic name is shared with 50+ unrelated creators, so it can discriminate
/// nothing. This is the same reason the conversation-start repository names its methods this
/// way.
#[async_trait]
pub trait RemoteAgentStopRequestRepository: Send + Sync {
    async fn create_stop_request(
        &self,
        request: RemoteAgentStopRequest,
    ) -> AppResult<RemoteAgentStopRequest>;

    async fn get_stop_request(&self, id: &str) -> AppResult<Option<RemoteAgentStopRequest>>;

    /// The dedupe read: the oldest UNSETTLED (`Pending`/`Stopping`) request for this
    /// conversation, if any. A second tap on Stop must join the in-flight brake rather than
    /// stack another one — an intent queue is not a click counter.
    async fn find_unsettled_stop_request_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<RemoteAgentStopRequest>>;

    /// Atomic CAS: select ONE `Pending` row (ORDER BY created_at ASC, id ASC), flip it to
    /// `Stopping` stamping claimed_at + updated_at, return it. At-most-one claimant: a
    /// concurrent call gets `None`. Transaction + `UPDATE … WHERE id=? AND status='pending'`
    /// guarded by rows-affected.
    async fn claim_pending_stop_request(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteAgentStopRequest>>;

    /// `Stopping` -> `Stopped`. Only applies while currently `Stopping` (guard in WHERE).
    async fn complete_stop_request(&self, id: &str, updated_at: DateTime<Utc>) -> AppResult<()>;

    /// `Stopping` -> `NoLiveRun`, the benign terminal. Only while currently `Stopping`.
    async fn resolve_stop_request_no_live_run(
        &self,
        id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()>;

    /// `Stopping` -> `Failed` + error_code. Only while currently `Stopping`.
    async fn fail_stop_request(
        &self,
        id: &str,
        error_code: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()>;

    /// Revoke-cancel: every `Pending` row for this device -> `Cancelled`. Returns count changed.
    async fn cancel_pending_stop_requests_for_device(
        &self,
        device_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64>;

    /// Startup sweep: `Stopping` rows claimed before the cutoff -> `FailedStale`. Never
    /// re-driven: a lost race between a dead claim and a re-drain would terminate a run the
    /// user has since restarted, so we fail closed and let the client retry explicitly.
    async fn fail_stale_stopping_stop_requests(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64>;
}
