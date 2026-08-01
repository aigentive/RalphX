use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::entities::{ChatConversationId, ProjectId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteConversationStartStatus {
    Pending,
    Starting,
    Started,
    Failed,
    Cancelled,
    FailedStale,
}

impl RemoteConversationStartStatus {
    /// Canonical DB/wire string for this status. Matches the camelCase serde
    /// representation so the persisted TEXT equals the serialized wire value.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            RemoteConversationStartStatus::Pending => "pending",
            RemoteConversationStartStatus::Starting => "starting",
            RemoteConversationStartStatus::Started => "started",
            RemoteConversationStartStatus::Failed => "failed",
            RemoteConversationStartStatus::Cancelled => "cancelled",
            RemoteConversationStartStatus::FailedStale => "failedStale",
        }
    }
}

impl fmt::Display for RemoteConversationStartStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_db_str())
    }
}

impl FromStr for RemoteConversationStartStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(RemoteConversationStartStatus::Pending),
            "starting" => Ok(RemoteConversationStartStatus::Starting),
            "started" => Ok(RemoteConversationStartStatus::Started),
            "failed" => Ok(RemoteConversationStartStatus::Failed),
            "cancelled" => Ok(RemoteConversationStartStatus::Cancelled),
            "failedStale" => Ok(RemoteConversationStartStatus::FailedStale),
            other => Err(format!("invalid RemoteConversationStartStatus: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConversationStartRequest {
    pub id: String,
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub content: String,
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: String,
    pub status: RemoteConversationStartStatus,
    pub error_code: Option<String>,
    pub requested_by_device_id: String,
    pub agent_run_id: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
