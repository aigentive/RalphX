use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteQueuedSendRequestStatus {
    Pending,
    Starting,
    Completed,
    Failed,
    FailedStale,
}

impl RemoteQueuedSendRequestStatus {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Starting => "starting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::FailedStale => "failedStale",
        }
    }
    pub fn is_settled(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::FailedStale)
    }
}
impl fmt::Display for RemoteQueuedSendRequestStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_db_str())
    }
}
impl FromStr for RemoteQueuedSendRequestStatus {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "starting" => Ok(Self::Starting),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "failedStale" => Ok(Self::FailedStale),
            other => Err(format!("invalid RemoteQueuedSendRequestStatus: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteQueuedSendRequest {
    pub id: String,
    pub conversation_id: String,
    pub queued_message_id: String,
    pub expected_active_run_id: Option<String>,
    pub status: RemoteQueuedSendRequestStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
