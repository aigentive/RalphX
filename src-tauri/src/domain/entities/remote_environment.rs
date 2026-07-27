// Remote environment registry entity (client mode, §6.1).
//
// One entity per paired remote host. `environment_id` is the HOST-reported identity
// (from its descriptor); `id` is the client-local row identity that also anchors the
// Keychain secret reference, so re-pairing the same host through a different URL keeps
// the same client-local id and the same Keychain entry.

use ralphx_remote_protocol::Scope;
use serde::{Deserialize, Serialize};

/// Client-local identity of a paired remote environment row.
///
/// Newtype so remote-environment ids cannot be confused with host `environment_id`
/// strings or other entity ids.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RemoteEnvironmentId(pub String);

impl RemoteEnvironmentId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RemoteEnvironmentId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RemoteEnvironmentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Reconciler-facing lifecycle status of a remote environment row (§6.1).
///
/// `PendingAdd` and `PendingDelete` are the staged add/remove states the startup
/// reconciler resolves; only `Active` environments are usable by the proxy surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteEnvironmentStatus {
    Active,
    PendingAdd,
    PendingDelete,
}

impl RemoteEnvironmentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::PendingAdd => "pending_add",
            Self::PendingDelete => "pending_delete",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "pending_add" => Some(Self::PendingAdd),
            "pending_delete" => Some(Self::PendingDelete),
            _ => None,
        }
    }
}

/// Builds the Keychain key for a remote environment's device token.
///
/// The key is anchored on the CLIENT-LOCAL row id, which survives upsert-dedup
/// re-pairs, so refreshing a token overwrites the same Keychain entry instead of
/// orphaning the previous one.
pub fn remote_environment_token_secret_ref(id: &RemoteEnvironmentId) -> String {
    format!("remote-env:{}:token", id.as_str())
}

/// A paired remote environment as stored in the client registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEnvironment {
    /// Client-local row identity (uuid).
    pub id: RemoteEnvironmentId,
    /// Host-reported identity from the environment descriptor. UNIQUE per registry.
    pub environment_id: String,
    /// User-facing display name.
    pub name: String,
    /// Preferred endpoint, e.g. `https://mac-studio.tailnet.ts.net`.
    pub base_url: String,
    /// Alternate endpoints for the same host (MagicDNS vs 100.x dedup, §6.1).
    pub candidate_urls: Vec<String>,
    /// Keychain key holding the device token — a reference, never the secret itself.
    pub token_secret_ref: String,
    /// Scopes the host granted at pairing time.
    pub scopes: Vec<Scope>,
    /// Protocol version the host reported at pairing time.
    pub protocol_version: u32,
    /// Staged add/remove lifecycle status.
    pub status: RemoteEnvironmentStatus,
    pub created_at: String,
    pub last_connected_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trips_through_db_strings() {
        for status in [
            RemoteEnvironmentStatus::Active,
            RemoteEnvironmentStatus::PendingAdd,
            RemoteEnvironmentStatus::PendingDelete,
        ] {
            assert_eq!(
                RemoteEnvironmentStatus::parse(status.as_str()),
                Some(status)
            );
        }
        assert_eq!(RemoteEnvironmentStatus::parse("half_paired"), None);
    }

    #[test]
    fn token_secret_ref_is_anchored_on_the_client_local_id() {
        let id = RemoteEnvironmentId::from_string("row-1");
        assert_eq!(remote_environment_token_secret_ref(&id), "remote-env:row-1:token");
    }

    #[test]
    fn status_serializes_snake_case_for_the_frontend() {
        let json = serde_json::to_string(&RemoteEnvironmentStatus::PendingAdd)
            .expect("status should serialize");
        assert_eq!(json, "\"pending_add\"");
    }
}
