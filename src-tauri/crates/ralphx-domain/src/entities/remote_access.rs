//! Remote-access (`:3849`) device credentials, pairing codes, sessions, and audit entries.
//!
//! Deliberately disjoint from [`crate::entities::api_key`]: those are :3848 bot credentials
//! with a permission bitmask, project scoping, and a zero-keys bootstrap bypass. Remote
//! devices are the owner's own UI clients — coarse scopes, whole-host, no bootstrap
//! exception (remote-multi-env §4.1, §4.5).

use ralphx_remote_protocol::Scope;
use serde::{Deserialize, Serialize};

/// A unique identifier for a paired remote device.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RemoteDeviceId(pub String);

/// A unique identifier for an outstanding pairing code row.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RemotePairingCodeId(pub String);

/// A unique identifier for a live remote session (one WS connection).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RemoteSessionId(pub String);

macro_rules! remote_id {
    ($name:ident) => {
        impl $name {
            /// Creates a new identifier with a random UUID v4.
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4().to_string())
            }

            /// Creates an identifier from an existing string (database deserialization).
            pub fn from_string(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Returns the inner string value.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

remote_id!(RemoteDeviceId);
remote_id!(RemotePairingCodeId);
remote_id!(RemoteSessionId);

/// An ordered, de-duplicated set of remote UI scopes.
///
/// Persisted as the `scopes` JSON array on `remote_devices` / `remote_pairing_codes`. Order
/// is canonical (declaration order of [`Scope`]) so the stored JSON is stable and two equal
/// grants always compare equal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RemoteScopeSet(Vec<Scope>);

impl RemoteScopeSet {
    /// The scopes a standard pairing grant carries: read + operate, never `ui:agent` (§4.4).
    pub fn default_pairing_grant() -> Self {
        Self::from_scopes([Scope::UiRead, Scope::UiOperate])
    }

    /// Builds a canonical set from any iterator, sorting and de-duplicating.
    pub fn from_scopes(scopes: impl IntoIterator<Item = Scope>) -> Self {
        let mut collected: Vec<Scope> = scopes.into_iter().collect();
        collected.sort();
        collected.dedup();
        Self(collected)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, scope: Scope) -> bool {
        self.0.contains(&scope)
    }

    pub fn as_slice(&self) -> &[Scope] {
        &self.0
    }

    pub fn to_vec(&self) -> Vec<Scope> {
        self.0.clone()
    }

    /// True when every scope in `self` is also present in `other`.
    pub fn is_subset_of(&self, other: &Self) -> bool {
        self.0.iter().all(|scope| other.contains(*scope))
    }

    /// Returns a copy with `scope` added.
    pub fn with(&self, scope: Scope) -> Self {
        Self::from_scopes(self.0.iter().copied().chain(std::iter::once(scope)))
    }

    /// Returns a copy with `scope` removed.
    pub fn without(&self, scope: Scope) -> Self {
        Self::from_scopes(self.0.iter().copied().filter(|entry| *entry != scope))
    }

    /// Serializes to the JSON array stored in the `scopes` column.
    pub fn to_json(&self) -> Result<String, RemoteScopeError> {
        serde_json::to_string(&self.0)
            .map_err(|error| RemoteScopeError::Malformed(error.to_string()))
    }

    /// Parses the JSON array stored in the `scopes` column.
    ///
    /// An unrecognized scope string is a hard error rather than a silent drop: a row written
    /// by a newer build must never be re-read as a narrower (or wider) grant.
    pub fn from_json(raw: &str) -> Result<Self, RemoteScopeError> {
        let parsed: Vec<Scope> = serde_json::from_str(raw)
            .map_err(|error| RemoteScopeError::Malformed(error.to_string()))?;
        Ok(Self::from_scopes(parsed))
    }
}

/// Typed failures around scope parsing and grant narrowing (rule 5 — no string matching).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RemoteScopeError {
    #[error("stored remote scope set is malformed: {0}")]
    Malformed(String),
    #[error("requested remote scope {0:?} is not part of the pairing grant")]
    NotGranted(Scope),
    #[error("remote scope {0:?} is not grantable in v1")]
    NotGrantable(Scope),
}

/// Scopes a pairing code may ever carry.
///
/// `ui:agent` is never minted into a pairing grant — it is a per-device host-side toggle
/// (§4.3, §5.4) — and `ui:elevated` is defined but ungrantable in v1 (§4.3).
pub fn validate_pairing_grant(grant: &RemoteScopeSet) -> Result<(), RemoteScopeError> {
    for scope in grant.as_slice() {
        match scope {
            Scope::UiRead | Scope::UiOperate => {}
            Scope::UiAgent | Scope::UiElevated => {
                return Err(RemoteScopeError::NotGrantable(*scope))
            }
        }
    }
    Ok(())
}

/// Resolves the scopes a pairing exchange mints for the new device.
///
/// `requested` must be a **subset** of the code's grant (§4.2); asking for more is an error,
/// not a silent intersection, so a client can never believe it holds a scope it does not.
pub fn effective_pairing_scopes(
    grant: &RemoteScopeSet,
    requested: Option<&RemoteScopeSet>,
) -> Result<RemoteScopeSet, RemoteScopeError> {
    let Some(requested) = requested else {
        return Ok(grant.clone());
    };
    for scope in requested.as_slice() {
        if !grant.contains(*scope) {
            return Err(RemoteScopeError::NotGranted(*scope));
        }
    }
    Ok(requested.clone())
}

/// A paired remote device — the durable credential record behind a `rxd_live_` token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDevice {
    pub id: RemoteDeviceId,
    pub name: String,
    /// SHA-256 hash of the raw `rxd_live_…` token. The raw token is never stored.
    #[serde(skip)]
    pub token_hash: String,
    /// Display-only prefix, e.g. `rxd_live_a3f2`.
    pub token_prefix: String,
    pub scopes: RemoteScopeSet,
    pub created_at: String,
    pub last_seen_at: Option<String>,
    /// `None` = active. Set before kill-channels fire (§4.4 teardown order).
    pub revoked_at: Option<String>,
}

impl RemoteDevice {
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }

    /// Whether the host owner has granted this device remote agent control.
    ///
    /// Derived from the live scope set rather than a separate column so introspection can
    /// never disagree with enforcement (§4.3 key point 7).
    pub fn agent_control_granted(&self) -> bool {
        self.scopes.contains(Scope::UiAgent)
    }
}

/// An outstanding (or spent) pairing code row. Only the hash is ever persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePairingCode {
    pub id: RemotePairingCodeId,
    #[serde(skip)]
    pub code_hash: String,
    pub scopes: RemoteScopeSet,
    pub created_at: String,
    pub expires_at: String,
    pub consumed_at: Option<String>,
}

/// Durable bookkeeping for one remote session.
///
/// The in-memory live-session registry — not this row — is the enforcement handle for
/// revocation teardown (§4.3 note).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSession {
    pub id: RemoteSessionId,
    pub device_id: RemoteDeviceId,
    pub connected_at: String,
    pub last_active_at: String,
    pub remote_addr: String,
    pub closed_at: Option<String>,
}

/// A single-use, device-bound WS upgrade ticket. Only the hash is persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteWsTicket {
    pub ticket_hash: String,
    pub device_id: RemoteDeviceId,
    pub expires_at: String,
    pub consumed_at: Option<String>,
}

/// Every auditable remote-access decision (§5.5).
///
/// An enum rather than free strings so call sites cannot drift (rule 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAuditAction {
    PairingCodeCreated,
    PairingCodeRevoked,
    PairingSucceeded,
    PairingRejected,
    AuthAccepted,
    AuthRejected,
    AuthStoreError,
    RateLimited,
    WsTicketIssued,
    WsTicketConsumed,
    WsTicketRejected,
    SessionOpened,
    SessionClosed,
    DeviceRevoked,
    AgentControlGranted,
    AgentControlRevoked,
    ListenerDisabled,
}

impl RemoteAuditAction {
    /// Stable string persisted in `remote_audit_log.action`.
    pub fn as_db_value(self) -> &'static str {
        match self {
            Self::PairingCodeCreated => "pairing_code_created",
            Self::PairingCodeRevoked => "pairing_code_revoked",
            Self::PairingSucceeded => "pairing_succeeded",
            Self::PairingRejected => "pairing_rejected",
            Self::AuthAccepted => "auth_accepted",
            Self::AuthRejected => "auth_rejected",
            Self::AuthStoreError => "auth_store_error",
            Self::RateLimited => "rate_limited",
            Self::WsTicketIssued => "ws_ticket_issued",
            Self::WsTicketConsumed => "ws_ticket_consumed",
            Self::WsTicketRejected => "ws_ticket_rejected",
            Self::SessionOpened => "session_opened",
            Self::SessionClosed => "session_closed",
            Self::DeviceRevoked => "device_revoked",
            Self::AgentControlGranted => "agent_control_granted",
            Self::AgentControlRevoked => "agent_control_revoked",
            Self::ListenerDisabled => "listener_disabled",
        }
    }
}

/// One row of `remote_audit_log`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAuditEntry {
    pub id: i64,
    pub device_id: Option<RemoteDeviceId>,
    pub action: String,
    pub detail: Option<String>,
    pub created_at: String,
}

#[cfg(test)]
#[path = "remote_access_tests.rs"]
mod tests;
