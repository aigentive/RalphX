use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteConversationLifecycleKind {
    Archive,
    Fork,
}
impl RemoteConversationLifecycleKind {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Archive => "archive",
            Self::Fork => "fork",
        }
    }
}
impl FromStr for RemoteConversationLifecycleKind {
    type Err = String;
    fn from_str(v: &str) -> Result<Self, String> {
        match v {
            "archive" => Ok(Self::Archive),
            "fork" => Ok(Self::Fork),
            _ => Err(format!("invalid remote conversation lifecycle kind: {v}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteConversationLifecycleStatus {
    Pending,
    Starting,
    Completed,
    Failed,
    FailedStale,
}
impl RemoteConversationLifecycleStatus {
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
impl fmt::Display for RemoteConversationLifecycleStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_db_str())
    }
}
impl FromStr for RemoteConversationLifecycleStatus {
    type Err = String;
    fn from_str(v: &str) -> Result<Self, String> {
        match v {
            "pending" => Ok(Self::Pending),
            "starting" => Ok(Self::Starting),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "failedStale" => Ok(Self::FailedStale),
            _ => Err(format!("invalid remote conversation lifecycle status: {v}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteConversationLifecycleRequest {
    pub id: String,
    pub kind: RemoteConversationLifecycleKind,
    pub conversation_id: String,
    pub close_pull_request: bool,
    pub allocated_conversation_id: Option<String>,
    pub status: RemoteConversationLifecycleStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
