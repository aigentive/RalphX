use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::entities::ChatConversationId;

/// Lifecycle of one remote STOP intent.
///
/// `NoLiveRun` is a BENIGN terminal, not an error: the brake was pulled and there was nothing
/// running to brake. Conflating it with `Failed` would make the common "the agent finished
/// between tap and drain" race look like a broken host, and would push the client into a retry
/// loop against a conversation that is already idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteAgentStopStatus {
    Pending,
    Stopping,
    Stopped,
    NoLiveRun,
    Failed,
    Cancelled,
    FailedStale,
}

impl RemoteAgentStopStatus {
    /// Canonical DB/wire string. Matches the camelCase serde representation so the persisted
    /// TEXT equals the serialized wire value.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            RemoteAgentStopStatus::Pending => "pending",
            RemoteAgentStopStatus::Stopping => "stopping",
            RemoteAgentStopStatus::Stopped => "stopped",
            RemoteAgentStopStatus::NoLiveRun => "noLiveRun",
            RemoteAgentStopStatus::Failed => "failed",
            RemoteAgentStopStatus::Cancelled => "cancelled",
            RemoteAgentStopStatus::FailedStale => "failedStale",
        }
    }

    /// Whether the request has settled. Terminal includes `NoLiveRun`.
    pub fn is_terminal(&self) -> bool {
        !matches!(
            self,
            RemoteAgentStopStatus::Pending | RemoteAgentStopStatus::Stopping
        )
    }
}

impl fmt::Display for RemoteAgentStopStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_db_str())
    }
}

impl FromStr for RemoteAgentStopStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(RemoteAgentStopStatus::Pending),
            "stopping" => Ok(RemoteAgentStopStatus::Stopping),
            "stopped" => Ok(RemoteAgentStopStatus::Stopped),
            "noLiveRun" => Ok(RemoteAgentStopStatus::NoLiveRun),
            "failed" => Ok(RemoteAgentStopStatus::Failed),
            "cancelled" => Ok(RemoteAgentStopStatus::Cancelled),
            "failedStale" => Ok(RemoteAgentStopStatus::FailedStale),
            other => Err(format!("invalid RemoteAgentStopStatus: {other}")),
        }
    }
}

/// A durable request to stop the agent running for one conversation.
///
/// The row carries NO process identity — no pid, no run id to target. Resolving what to
/// terminate is host-owned and happens at drain time, so a client can never name a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAgentStopRequest {
    pub id: String,
    pub conversation_id: ChatConversationId,
    pub status: RemoteAgentStopStatus,
    pub error_code: Option<String>,
    pub requested_by_device_id: String,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
