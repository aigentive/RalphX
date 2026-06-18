use crate::entities::types::ApiKeyId;
use serde::{Deserialize, Serialize};

/// A single entry from the API audit log
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub id: i64,
    pub api_key_id: String,
    pub tool_name: String,
    pub project_id: Option<String>,
    pub success: bool,
    pub latency_ms: Option<i64>,
    pub created_at: String,
}

/// Bitmask permission constants
pub const PERMISSION_READ: i32 = 1;
pub const PERMISSION_WRITE: i32 = 2;
pub const PERMISSION_ADMIN: i32 = 4;
pub const PERMISSION_CREATE_PROJECT: i32 = 8;
/// Maximum valid permissions value (all bits set: read | write | admin | create_project)
pub const PERMISSION_MAX: i32 = 15;

/// An API key for external access to RalphX
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: ApiKeyId,
    pub name: String,
    /// SHA-256 hash of the full key (never store raw)
    pub key_hash: String,
    /// First 12 chars of the key for display (e.g. "rxk_live_a3f2")
    pub key_prefix: String,
    /// Bitmask: 1=read, 2=write, 4=admin (default: 3 = read+write)
    pub permissions: i32,
    pub created_at: String,
    /// None = active, Some = revoked
    pub revoked_at: Option<String>,
    /// Updated on each validated request
    pub last_used_at: Option<String>,
    /// Grace period end time after key rotation (old key remains valid until this)
    pub grace_expires_at: Option<String>,
    pub metadata: Option<String>,
}

impl ApiKey {
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }

    pub fn is_in_grace_period(&self) -> bool {
        if let Some(grace_expires_at) = &self.grace_expires_at {
            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            grace_expires_at > &now
        } else {
            false
        }
    }

    pub fn has_permission(&self, permission: i32) -> bool {
        self.permissions & permission != 0
    }
}
