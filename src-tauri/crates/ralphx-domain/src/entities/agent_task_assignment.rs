use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::{
    AgentRunId, AgentTaskDetail, AgentTaskId, AgentTaskListId, DelegatedSessionId, TeamMemberId,
    TeamSessionId,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentTaskAssignmentId(pub String);

impl AgentTaskAssignmentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for AgentTaskAssignmentId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AgentTaskAssignmentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskAssignmentState {
    Reserved,
    Active,
    CompletionRequested,
    ReleaseRequested,
    Completed,
    Released,
    Failed,
    Cancelled,
}

impl AgentTaskAssignmentState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Active => "active",
            Self::CompletionRequested => "completion_requested",
            Self::ReleaseRequested => "release_requested",
            Self::Completed => "completed",
            Self::Released => "released",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_unresolved(self) -> bool {
        matches!(
            self,
            Self::Reserved | Self::Active | Self::CompletionRequested | Self::ReleaseRequested
        )
    }
}

impl fmt::Display for AgentTaskAssignmentState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.as_str())
    }
}

impl std::str::FromStr for AgentTaskAssignmentState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "reserved" => Ok(Self::Reserved),
            "active" => Ok(Self::Active),
            "completion_requested" => Ok(Self::CompletionRequested),
            "release_requested" => Ok(Self::ReleaseRequested),
            "completed" => Ok(Self::Completed),
            "released" => Ok(Self::Released),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("Invalid agent task assignment state: {value}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentTaskAssignment {
    pub id: AgentTaskAssignmentId,
    pub delegated_session_id: DelegatedSessionId,
    pub attempt_number: i64,
    pub caller_agent_run_id: AgentRunId,
    pub planned_delegated_agent_run_id: Option<AgentRunId>,
    pub delegated_agent_run_id: Option<AgentRunId>,
    /// Team linkage is additive so legacy assignments remain valid.
    pub team_id: Option<TeamSessionId>,
    pub team_member_id: Option<TeamMemberId>,
    pub team_member_generation: Option<i64>,
    pub task_list_id: AgentTaskListId,
    pub task_id: AgentTaskId,
    pub delegate_agent_name: String,
    pub state: AgentTaskAssignmentState,
    pub prior_owner_agent: Option<String>,
    pub settlement_reason: Option<String>,
    pub completion_metadata: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub run_bound_at: Option<DateTime<Utc>>,
    pub settled_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AgentTaskAssignmentView {
    pub assignment: AgentTaskAssignment,
    pub caller_scope_type: String,
    pub caller_scope_id: String,
    pub task: AgentTaskDetail,
}

#[derive(Debug, Clone)]
pub struct AgentTaskAssignmentReservation {
    pub assignment: AgentTaskAssignmentView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTaskAssignmentTerminalStatus {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct AgentTaskAssignmentSettlement {
    pub assignment: AgentTaskAssignmentView,
    pub task_reopened: bool,
    pub task_completed: bool,
}
