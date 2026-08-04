use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

use crate::entities::{ProjectId, TaskId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteResumeRequestStatus {
    Pending,
    Starting,
    Completed,
    Failed,
    FailedStale,
}

impl RemoteResumeRequestStatus {
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

impl fmt::Display for RemoteResumeRequestStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_db_str())
    }
}

impl FromStr for RemoteResumeRequestStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "starting" => Ok(Self::Starting),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "failedStale" => Ok(Self::FailedStale),
            other => Err(format!("invalid RemoteResumeRequestStatus: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteTaskAction {
    Resume,
    Restart,
    GroupResume,
}

impl RemoteTaskAction {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Resume => "resume",
            Self::Restart => "restart",
            Self::GroupResume => "groupResume",
        }
    }
}

impl FromStr for RemoteTaskAction {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "resume" => Ok(Self::Resume),
            "restart" => Ok(Self::Restart),
            "groupResume" => Ok(Self::GroupResume),
            other => Err(format!("invalid RemoteTaskAction: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteExecutionResumeRequest {
    pub id: String,
    pub project_id: Option<ProjectId>,
    pub status: RemoteResumeRequestStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteTaskActionRequest {
    pub id: String,
    pub action: RemoteTaskAction,
    pub task_id: Option<TaskId>,
    pub project_id: ProjectId,
    pub group_kind: Option<String>,
    pub group_id: Option<String>,
    pub force: bool,
    pub note: Option<String>,
    pub status: RemoteResumeRequestStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
