use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::entities::{AgentConversationWorkspaceMode, ChatConversationId, ProjectId};

/// Lifecycle of one remote MODE SWITCH intent (WP5a).
///
/// `AlreadyInMode` is a BENIGN terminal, not an error: the conversation was already in the
/// requested mode, so there was nothing to switch. Conflating it with `Failed` would make the
/// common "two devices picked the same mode" and "the picker re-fired on rehydrate" races look
/// like a broken host and push the client into a retry loop against a conversation that is
/// already exactly where the user wanted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteConversationModeSwitchStatus {
    Pending,
    Switching,
    Switched,
    AlreadyInMode,
    Failed,
    Cancelled,
    FailedStale,
}

impl RemoteConversationModeSwitchStatus {
    /// Canonical DB/wire string. Matches the camelCase serde representation so the persisted
    /// TEXT equals the serialized wire value.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            RemoteConversationModeSwitchStatus::Pending => "pending",
            RemoteConversationModeSwitchStatus::Switching => "switching",
            RemoteConversationModeSwitchStatus::Switched => "switched",
            RemoteConversationModeSwitchStatus::AlreadyInMode => "alreadyInMode",
            RemoteConversationModeSwitchStatus::Failed => "failed",
            RemoteConversationModeSwitchStatus::Cancelled => "cancelled",
            RemoteConversationModeSwitchStatus::FailedStale => "failedStale",
        }
    }

    /// Whether the request has settled. Terminal includes `AlreadyInMode`.
    pub fn is_terminal(&self) -> bool {
        !matches!(
            self,
            RemoteConversationModeSwitchStatus::Pending
                | RemoteConversationModeSwitchStatus::Switching
        )
    }
}

impl fmt::Display for RemoteConversationModeSwitchStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_db_str())
    }
}

impl FromStr for RemoteConversationModeSwitchStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(RemoteConversationModeSwitchStatus::Pending),
            "switching" => Ok(RemoteConversationModeSwitchStatus::Switching),
            "switched" => Ok(RemoteConversationModeSwitchStatus::Switched),
            "alreadyInMode" => Ok(RemoteConversationModeSwitchStatus::AlreadyInMode),
            "failed" => Ok(RemoteConversationModeSwitchStatus::Failed),
            "cancelled" => Ok(RemoteConversationModeSwitchStatus::Cancelled),
            "failedStale" => Ok(RemoteConversationModeSwitchStatus::FailedStale),
            other => Err(format!(
                "invalid RemoteConversationModeSwitchStatus: {other}"
            )),
        }
    }
}

/// A durable request to move one project agent conversation to a different workspace mode.
///
/// The row carries the TARGET MODE and nothing else about how to get there: no base ref, no
/// branch mode, no runtime override, no source pull request. Every one of those steers real
/// workspace preparation (`GitService::ref_exists`, `ensure_git_worktree`) and is host-resolved
/// at drain time, so a client cannot aim workspace creation at a ref it names. Field absence,
/// not a pinned value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConversationModeSwitchRequest {
    pub id: String,
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub target_mode: AgentConversationWorkspaceMode,
    pub status: RemoteConversationModeSwitchStatus,
    pub error_code: Option<String>,
    pub requested_by_device_id: String,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
