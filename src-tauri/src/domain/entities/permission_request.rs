//! Pending permission-request records shared by the UI permission bridge and
//! the permission repositories.
//!
//! These are the wire records the MCP permission bridge persists and the UI
//! resolves. The live coordination container (`PermissionState`, watch channels)
//! stays in the application layer; only the records and their TTL policy live here.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How long a pending permission request stays actionable before it is treated
/// as abandoned by both the durable read path and the notification producer.
pub const PERMISSION_REQUEST_TTL: Duration = Duration::from_secs(300);
/// Permission decision made by the user in the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDecision {
    pub decision: String, // "allow" or "deny"
    pub message: Option<String>,
}

/// Metadata for a pending permission request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPermissionInfo {
    pub request_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub context: Option<String>,
    // Agent identity fields (optional for backward compat)
    pub agent_type: Option<String>,
    pub task_id: Option<String>,
    pub context_type: Option<String>,
    pub context_id: Option<String>,
    #[serde(default = "default_created_at")]
    pub created_at: String,
}

fn default_created_at() -> String {
    Utc::now().to_rfc3339()
}

pub fn is_within_permission_request_ttl(created_at: &str) -> bool {
    let Ok(created_at) = DateTime::parse_from_rfc3339(created_at) else {
        return false;
    };
    let ttl = chrono::Duration::seconds(PERMISSION_REQUEST_TTL.as_secs() as i64);
    created_at.with_timezone(&Utc) + ttl > Utc::now()
}
