use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{AgentRunId, AgentTaskAssignmentId, DelegatedSessionId, TeamSessionId};
use crate::agents::{AgentHarnessKind, LogicalEffort};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamMemberId(pub String);
impl TeamMemberId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl Default for TeamMemberId {
    fn default() -> Self {
        Self::new()
    }
}
impl std::fmt::Display for TeamMemberId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn normalize_team_member_name(name: &str) -> Result<String, String> {
    let normalized = name
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if normalized.is_empty()
        || normalized.len() > 96
        || !normalized
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
    {
        return Err(
            "team member name must be 1-96 ASCII letters, digits, spaces, hyphens, or underscores"
                .to_string(),
        );
    }
    Ok(normalized)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMemberStatus {
    Provisioning,
    Idle,
    Working,
    AwaitingInput,
    AwaitingApproval,
    Stopping,
    Suspended,
    Failed,
    Stopped,
}
impl TeamMemberStatus {
    pub fn is_terminal(self) -> bool {
        self == Self::Stopped
    }
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Provisioning,
                Self::Idle | Self::Failed | Self::Stopped
            ) | (
                Self::Idle,
                Self::Working | Self::Suspended | Self::Stopping | Self::Failed
            ) | (
                Self::Working,
                Self::Idle
                    | Self::AwaitingInput
                    | Self::AwaitingApproval
                    | Self::Stopping
                    | Self::Failed
            ) | (
                Self::AwaitingInput | Self::AwaitingApproval,
                Self::Working | Self::Stopping | Self::Failed
            ) | (Self::Stopping, Self::Stopped | Self::Idle | Self::Failed)
                | (Self::Suspended, Self::Idle | Self::Stopping | Self::Failed)
                | (
                    Self::Failed,
                    Self::Idle | Self::Suspended | Self::Stopping | Self::Stopped
                )
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub id: TeamMemberId,
    pub team_id: TeamSessionId,
    pub normalized_name: String,
    pub name: String,
    pub canonical_agent_name: String,
    pub role_summary: String,
    pub harness: Option<AgentHarnessKind>,
    pub logical_model: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
    pub delegated_session_id: Option<DelegatedSessionId>,
    pub generation: i64,
    pub current_run_id: Option<AgentRunId>,
    pub current_assignment_id: Option<AgentTaskAssignmentId>,
    pub status: TeamMemberStatus,
    pub last_activity_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub stopped_at: Option<DateTime<Utc>>,
}
impl TeamMember {
    pub fn validate_name(&self) -> Result<(), String> {
        if normalize_team_member_name(&self.name)? == self.normalized_name {
            Ok(())
        } else {
            Err("team member normalized name does not match display name".to_string())
        }
    }
    pub fn is_current_generation(&self, generation: i64) -> bool {
        self.generation == generation
    }
    pub fn current_run_is_authoritative(&self, generation: i64, run_id: &AgentRunId) -> bool {
        self.is_current_generation(generation) && self.current_run_id.as_ref() == Some(run_id)
    }
    pub fn replace_runtime(
        &mut self,
        delegated_session_id: DelegatedSessionId,
        now: DateTime<Utc>,
    ) {
        self.generation += 1;
        self.delegated_session_id = Some(delegated_session_id);
        self.current_run_id = None;
        self.current_assignment_id = None;
        self.updated_at = now;
    }
    pub fn transition_to(
        &mut self,
        next: TeamMemberStatus,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        if !self.status.can_transition_to(next) {
            return Err(format!(
                "invalid team member transition: {:?} -> {:?}",
                self.status, next
            ));
        }
        self.status = next;
        self.updated_at = now;
        if next == TeamMemberStatus::Stopped {
            self.stopped_at = Some(now);
        }
        Ok(())
    }
}
