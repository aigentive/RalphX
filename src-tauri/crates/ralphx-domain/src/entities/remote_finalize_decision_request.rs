use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

use crate::entities::IdeationSessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteFinalizeDecision {
    Accept,
    Reject,
}

impl RemoteFinalizeDecision {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Reject => "reject",
        }
    }
}

impl FromStr for RemoteFinalizeDecision {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "accept" => Ok(Self::Accept),
            "reject" => Ok(Self::Reject),
            other => Err(format!("invalid RemoteFinalizeDecision: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteFinalizeDecisionRequestStatus {
    Pending,
    Starting,
    Completed,
    Failed,
    FailedStale,
}

impl RemoteFinalizeDecisionRequestStatus {
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
impl fmt::Display for RemoteFinalizeDecisionRequestStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_db_str())
    }
}
impl FromStr for RemoteFinalizeDecisionRequestStatus {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "starting" => Ok(Self::Starting),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "failedStale" => Ok(Self::FailedStale),
            other => Err(format!(
                "invalid RemoteFinalizeDecisionRequestStatus: {other}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteFinalizeDecisionRequest {
    pub id: String,
    pub session_id: IdeationSessionId,
    pub decision: RemoteFinalizeDecision,
    pub status: RemoteFinalizeDecisionRequestStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
