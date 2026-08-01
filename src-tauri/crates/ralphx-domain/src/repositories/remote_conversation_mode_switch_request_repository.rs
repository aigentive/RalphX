use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::{ChatConversationId, RemoteConversationModeSwitchRequest};
use crate::error::AppResult;

/// Durable store for remote MODE SWITCH intents (WP5a).
///
/// Method names are deliberately DISTINCTIVE (`create_mode_switch_request`,
/// `claim_pending_mode_switch_request`, …) rather than the generic `create`/`update` used
/// elsewhere: the remote authority audit's detector (b) matches call-graph tokens against
/// write-site markers, and a generic name is shared with 50+ unrelated creators, so it can
/// discriminate nothing. This is the same reason the conversation-start, conversation-message,
/// and agent-stop repositories name their methods this way, and the markers are pairwise
/// distinct so the four intent surfaces can never be confused for one another.
#[async_trait]
pub trait RemoteConversationModeSwitchRequestRepository: Send + Sync {
    async fn create_mode_switch_request(
        &self,
        request: RemoteConversationModeSwitchRequest,
    ) -> AppResult<RemoteConversationModeSwitchRequest>;

    async fn get_mode_switch_request(
        &self,
        id: &str,
    ) -> AppResult<Option<RemoteConversationModeSwitchRequest>>;

    /// The dedupe read: the oldest UNSETTLED (`Pending`/`Switching`) request for this
    /// conversation, if any. A repeat pick of the SAME mode joins it; a pick of a DIFFERENT mode
    /// is refused rather than replacing it, because the dispatcher may already have claimed the
    /// in-flight row and begun preparing a worktree for the mode it named.
    async fn find_unsettled_mode_switch_request_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<RemoteConversationModeSwitchRequest>>;

    /// Atomic CAS: select ONE `Pending` row (ORDER BY created_at ASC, id ASC), flip it to
    /// `Switching` stamping claimed_at + updated_at, return it. At-most-one claimant: a
    /// concurrent call gets `None`. Transaction + `UPDATE … WHERE id=? AND status='pending'`
    /// guarded by rows-affected.
    async fn claim_pending_mode_switch_request(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteConversationModeSwitchRequest>>;

    /// `Switching` -> `Switched`. Only applies while currently `Switching` (guard in WHERE).
    async fn complete_mode_switch_request(
        &self,
        id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()>;

    /// `Switching` -> `AlreadyInMode`, the benign terminal. Only while currently `Switching`.
    /// Reached when the conversation moved into the requested mode between persist and drain.
    async fn resolve_mode_switch_request_already_in_mode(
        &self,
        id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()>;

    /// `Switching` -> `Failed` + error_code. Only while currently `Switching`.
    async fn fail_mode_switch_request(
        &self,
        id: &str,
        error_code: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()>;

    /// Revoke-cancel: every `Pending` row for this device -> `Cancelled`. Returns count changed.
    async fn cancel_pending_mode_switch_requests_for_device(
        &self,
        device_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64>;

    /// Startup sweep: `Switching` rows claimed before the cutoff -> `FailedStale`. Never
    /// re-driven: mode switching prepares worktrees and can cross the plan/review boundary, so a
    /// lost race between a dead claim and a re-drain could tear down review state for a mode the
    /// user has since left. Fail closed and let the client retry explicitly.
    async fn fail_stale_switching_mode_switch_requests(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64>;
}
