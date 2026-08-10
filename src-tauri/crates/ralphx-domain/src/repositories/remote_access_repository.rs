//! Repository traits for the :3849 remote-access auth tables (§4.3).
//!
//! Every implementation must go through `DbConnection::run` / `run_transaction` (rule 16).
//! The lookup and consume methods return **typed tri-state outcomes** rather than
//! `Option`, so a store failure can never be observed as "no row" — the distinction is what
//! makes the bearer middleware fail closed with 500 instead of an anonymous 401 (§4.4).

use async_trait::async_trait;

use crate::entities::{
    RemoteAuditAction, RemoteAuditEntry, RemoteDevice, RemoteDeviceId, RemotePairingCode,
    RemotePairingCodeId, RemoteScopeSet, RemoteSession, RemoteSessionId,
};
use crate::error::AppResult;
use ralphx_remote_protocol::Scope;

/// Result of resolving a presented bearer token against `remote_devices`.
///
/// `Err(AppError)` from the repository means the store failed; it is deliberately NOT a
/// variant here so callers cannot pattern-match a store outage into a rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteDeviceLookup {
    /// A live, non-revoked device.
    Active(RemoteDevice),
    /// The token matched a device whose `revoked_at` is set.
    Revoked(RemoteDevice),
    /// No row carries this token hash.
    Unknown,
}

/// Everything needed to mint a device inside the pairing-code consume transaction.
#[derive(Debug, Clone)]
pub struct RemotePairingRedemption {
    pub code_hash: String,
    pub device_id: RemoteDeviceId,
    pub device_name: String,
    pub token_hash: String,
    pub token_prefix: String,
    /// `None` asks for the code's whole grant; `Some` must be a subset of it (§4.2).
    pub requested_scopes: Option<RemoteScopeSet>,
    /// RFC3339 timestamp used for both the expiry comparison and the written rows.
    pub now: String,
    /// Detail for the `pairing_succeeded` audit row, written **inside** the same
    /// transaction as the device mint.
    ///
    /// It lives here rather than in the handler because a post-commit audit write that fails
    /// leaves a consumed code plus an active device whose token was never delivered — an
    /// unusable row the owner has to clean up by hand. Mint and trail commit together or not
    /// at all.
    pub audit_detail: Option<String>,
}

/// Outcome of redeeming a pairing code. Exactly one concurrent redemption can be `Paired`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemotePairingOutcome {
    Paired(RemoteDevice),
    /// No pairing-code row carries this hash.
    Unknown,
    /// `expires_at` is at or before `now`.
    Expired,
    /// `consumed_at` was already set — single-use enforced (P-7).
    AlreadyConsumed,
    /// The request asked for a scope outside the code's grant.
    ScopeNotGranted(Scope),
}

/// Outcome of consuming a WS ticket at upgrade time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteWsTicketOutcome {
    /// Valid, unconsumed, unexpired — now consumed and bound to this device.
    Consumed(RemoteDeviceId),
    Unknown,
    Expired,
    AlreadyConsumed,
}

/// Paired devices: the credential store behind every authenticated remote request.
#[async_trait]
pub trait RemoteDeviceRepository: Send + Sync {
    /// Resolves a SHA-256 token hash. Errors are store failures, never "absent".
    async fn lookup_by_token_hash(&self, token_hash: &str) -> AppResult<RemoteDeviceLookup>;

    async fn get(&self, id: &RemoteDeviceId) -> AppResult<Option<RemoteDevice>>;

    /// All devices, newest first, including revoked ones (the host UI shows both).
    async fn list(&self) -> AppResult<Vec<RemoteDevice>>;

    /// Sets `revoked_at` if it is not already set. Returns the device as it now stands.
    ///
    /// Idempotent: re-revoking is not an error, so recovery paths can repeat it safely.
    async fn revoke(&self, id: &RemoteDeviceId, now: &str) -> AppResult<Option<RemoteDevice>>;

    /// Replaces the device's scope set. Used by the agent-control toggle (§5.4).
    ///
    /// Refuses revoked devices so a toggle can never widen a dead credential.
    async fn set_scopes(
        &self,
        id: &RemoteDeviceId,
        scopes: &RemoteScopeSet,
    ) -> AppResult<Option<RemoteDevice>>;

    /// Updates `last_seen_at` on every authenticated request (§4.3).
    async fn touch_last_seen(&self, id: &RemoteDeviceId, now: &str) -> AppResult<()>;
}

/// Single-use pairing codes.
#[async_trait]
pub trait RemotePairingCodeRepository: Send + Sync {
    async fn create(&self, code: RemotePairingCode) -> AppResult<RemotePairingCode>;

    /// Validates and consumes a code, minting the device **and its audit row** in the same
    /// transaction.
    ///
    /// `BEGIN IMMEDIATE` plus a guarded `consumed_at IS NULL` update means two concurrent
    /// redemptions of one code can never both succeed (P-7).
    async fn redeem(&self, redemption: RemotePairingRedemption) -> AppResult<RemotePairingOutcome>;

    /// Codes that are neither consumed nor expired as of `now`.
    async fn list_outstanding(&self, now: &str) -> AppResult<Vec<RemotePairingCode>>;

    /// Marks a code consumed without pairing, so the host UI can cancel an outstanding code.
    async fn cancel(&self, id: &RemotePairingCodeId, now: &str) -> AppResult<bool>;
}

/// Durable session bookkeeping. The live registry, not this table, enforces teardown.
#[async_trait]
pub trait RemoteSessionRepository: Send + Sync {
    async fn open(&self, session: RemoteSession) -> AppResult<RemoteSession>;

    async fn touch(&self, id: &RemoteSessionId, now: &str) -> AppResult<()>;

    async fn close(&self, id: &RemoteSessionId, now: &str) -> AppResult<()>;

    /// Closes every open session for a device; returns how many rows were closed.
    async fn close_all_for_device(&self, device_id: &RemoteDeviceId, now: &str)
        -> AppResult<usize>;

    /// Closes every open session on the host (listener disable).
    async fn close_all(&self, now: &str) -> AppResult<usize>;

    async fn list_open(&self) -> AppResult<Vec<RemoteSession>>;
}

/// Device-bound, single-use WS upgrade tickets.
#[async_trait]
pub trait RemoteWsTicketRepository: Send + Sync {
    async fn issue(
        &self,
        ticket_hash: &str,
        device_id: &RemoteDeviceId,
        expires_at: &str,
    ) -> AppResult<()>;

    /// Validates and consumes in one transaction, so a replay always loses.
    async fn consume(&self, ticket_hash: &str, now: &str) -> AppResult<RemoteWsTicketOutcome>;

    /// Invalidates every outstanding ticket for a device (revocation, agent-control off).
    async fn consume_all_for_device(
        &self,
        device_id: &RemoteDeviceId,
        now: &str,
    ) -> AppResult<usize>;
}

/// Append-only audit trail for every remote auth decision (§5.5).
#[async_trait]
pub trait RemoteAuditLogRepository: Send + Sync {
    async fn record(
        &self,
        device_id: Option<&RemoteDeviceId>,
        action: RemoteAuditAction,
        detail: Option<&str>,
        now: &str,
    ) -> AppResult<()>;

    /// Most recent first. Named `list_recent` so it does not collide with
    /// [`RemoteDeviceRepository::list`] on a type implementing both.
    async fn list_recent(&self, limit: Option<i64>) -> AppResult<Vec<RemoteAuditEntry>>;

    /// Drops rows older than `cutoff` (RFC3339), returning how many were removed.
    ///
    /// The log is append-only on the request path, so without retention it grows for the life
    /// of the install inside the main app database. Reads are capped at 1000 rows; writes need
    /// a ceiling too.
    async fn prune_before(&self, cutoff: &str) -> AppResult<usize>;
}
