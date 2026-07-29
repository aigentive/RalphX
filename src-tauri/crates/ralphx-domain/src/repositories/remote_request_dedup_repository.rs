//! Repository traits for remote request idempotency and attachment metadata (§4.3, C-16).
//!
//! Every implementation must go through `DbConnection::run` / `run_transaction` (rule 16).
//!
//! `lookup` returns a **typed tri-state**, never `Option`, for the same reason
//! `RemoteDeviceLookup` does: a store outage must not be observable as "no record", because
//! "no record" is exactly the state that PERMITS execution. Collapsing the two would turn a
//! transient DB failure into a duplicate side effect (stateful-workflow: fail closed on reads).

use async_trait::async_trait;

use crate::entities::{RemoteAttachment, RemoteDeviceId, RemoteRequestDedupRecord};
use crate::error::AppResult;

/// Result of consulting the durable dedup table for `(device_id, request_id)`.
///
/// `Err(AppError)` from the repository means the store failed; it is deliberately NOT a
/// variant here so callers cannot pattern-match a store outage into "execute it again".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteRequestDedupLookup {
    /// A row exists and has not passed its TTL — replay it.
    Fresh(RemoteRequestDedupRecord),
    /// A row exists but has passed its TTL — treat as a new request.
    Expired,
    /// The read succeeded and there is genuinely no row.
    Absent,
}

#[async_trait]
pub trait RemoteRequestDedupRepository: Send + Sync {
    /// Resolves a completed-outcome record. `now` is RFC3339 and drives the TTL comparison.
    async fn lookup(
        &self,
        device_id: &RemoteDeviceId,
        request_id: &str,
        now: &str,
    ) -> AppResult<RemoteRequestDedupLookup>;

    /// Upserts the completed outcome keyed on `(device_id, request_id)`.
    ///
    /// Idempotent: re-recording the same id+hash overwrites with identical content, so a
    /// retried record write after a partial failure is safe.
    async fn record(&self, record: RemoteRequestDedupRecord) -> AppResult<()>;

    /// Deletes every row whose `expires_at` is at or before `now`. Returns the row count.
    async fn purge_expired(&self, now: &str) -> AppResult<usize>;
}

#[async_trait]
pub trait RemoteAttachmentRepository: Send + Sync {
    async fn record(&self, attachment: RemoteAttachment) -> AppResult<()>;

    /// Atomically inserts the attachment only when the device's resulting usage is within
    /// `quota_bytes`. Returns `false` when the reservation would exceed the quota.
    async fn record_within_device_quota(
        &self,
        attachment: RemoteAttachment,
        quota_bytes: i64,
    ) -> AppResult<bool>;

    /// Device-scoped fetch. The device id is part of the query, not a post-filter, so a
    /// cross-device read can never be served by forgetting a check at the call site.
    async fn get_for_device(
        &self,
        device_id: &RemoteDeviceId,
        id: &str,
    ) -> AppResult<Option<RemoteAttachment>>;

    /// `SUM(size)` for one device, in bytes. Integer arithmetic only — never float.
    async fn device_usage_bytes(&self, device_id: &RemoteDeviceId) -> AppResult<i64>;
}
