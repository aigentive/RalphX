//! Durable request-idempotency and attachment records for the :3849 remote facade (§4.3, C-16).
//!
//! Two independent stores share this module because they share a migration and a lifetime:
//! both are per-device, host-owned, and never surface to the local UI.
//!
//! The dedup record is the DURABLE half of the two-layer design. The in-memory reservation
//! (`DashMap`) coalesces concurrent duplicates within one process lifetime; this record is what
//! survives a host restart. It is deliberately keyed `(device_id, request_id)` so one device
//! cannot observe — or collide with — another device's request ids.

use serde::{Deserialize, Serialize};

use super::remote_access::RemoteDeviceId;

/// Which half of `DispatchOutcome` a cached response replays as.
///
/// A command-level `Err` IS a completed outcome: the command ran, decided, and returned a
/// business error. Replaying it must return the same error rather than re-executing, otherwise
/// a client retry after a failed edit would execute the edit twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteDedupOutcomeKind {
    Ok,
    Err,
}

impl RemoteDedupOutcomeKind {
    pub fn as_column(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Err => "err",
        }
    }

    /// Strict parse: an unrecognised column value is a store error, never a silent `Ok`.
    pub fn from_column(raw: &str) -> Option<Self> {
        match raw {
            "ok" => Some(Self::Ok),
            "err" => Some(Self::Err),
            _ => None,
        }
    }
}

/// One completed, replayable remote request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRequestDedupRecord {
    pub device_id: RemoteDeviceId,
    pub request_id: String,
    /// SHA-256 over `cmd` and the raw request `args` bytes; binds the id to its payload so a
    /// client reusing an id for different work is rejected instead of served a wrong cache hit.
    pub args_hash: String,
    pub outcome: RemoteDedupOutcomeKind,
    /// The serialized response envelope body, replayed verbatim.
    pub response: String,
    pub created_at: String,
    pub expires_at: String,
}

/// One uploaded attachment's metadata row.
///
/// `display_name` is client-supplied and is stored HERE precisely so it never becomes a path
/// component: the only path component is `id`, a server-minted UUID (CodeQL path-safety).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAttachment {
    pub id: String,
    pub device_id: RemoteDeviceId,
    pub display_name: Option<String>,
    pub mime: String,
    pub size: i64,
    pub created_at: String,
}
