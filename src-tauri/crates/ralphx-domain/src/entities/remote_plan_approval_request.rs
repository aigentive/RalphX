use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

use crate::entities::IdeationSessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemotePlanApprovalRequestStatus {
    Pending,
    Starting,
    Completed,
    Failed,
    FailedStale,
}

impl RemotePlanApprovalRequestStatus {
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
impl fmt::Display for RemotePlanApprovalRequestStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_db_str())
    }
}
impl FromStr for RemotePlanApprovalRequestStatus {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "starting" => Ok(Self::Starting),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "failedStale" => Ok(Self::FailedStale),
            other => Err(format!("invalid RemotePlanApprovalRequestStatus: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemotePlanApprovalRequest {
    pub id: String,
    pub session_id: IdeationSessionId,
    pub artifact_id: String,
    pub blueprint_artifact_id: Option<String>,
    pub blueprint_artifact_version: Option<u32>,
    pub status: RemotePlanApprovalRequestStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
