use super::{
    AgentRunId, AgentTaskAssignmentId, ChatConversationId, DelegatedSessionId, TeamMemberId,
    TeamSessionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamRunBindingId(pub String);
impl TeamRunBindingId {
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
impl Default for TeamRunBindingId {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRunTriggerKind {
    UserCoordinatorTurn,
    Assignment,
    DirectMessage,
    WakeBatch,
    Retry,
    Control,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamWorkClassification {
    CoordinationOnly,
    ReadOnly,
    Write,
    Validator,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRunBindingStatus {
    Planned,
    Launching,
    Running,
    Terminal,
    Cancelled,
    Failed,
}
impl TeamRunBindingStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Planned,
                Self::Launching | Self::Cancelled | Self::Failed
            ) | (
                Self::Launching,
                Self::Running | Self::Cancelled | Self::Failed
            ) | (
                Self::Running,
                Self::Terminal | Self::Cancelled | Self::Failed
            )
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamRunBinding {
    pub id: TeamRunBindingId,
    pub team_id: TeamSessionId,
    pub team_member_id: Option<TeamMemberId>,
    pub team_member_generation: Option<i64>,
    pub agent_run_id: AgentRunId,
    pub conversation_id: ChatConversationId,
    pub delegated_session_id: Option<DelegatedSessionId>,
    pub trigger_kind: TeamRunTriggerKind,
    pub work_classification: TeamWorkClassification,
    pub assignment_id: Option<AgentTaskAssignmentId>,
    pub first_message_sequence: Option<i64>,
    pub last_message_sequence: Option<i64>,
    pub status: TeamRunBindingStatus,
    pub version: i64,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub launched_at: Option<DateTime<Utc>>,
    pub terminal_at: Option<DateTime<Utc>>,
}
impl TeamRunBinding {
    pub fn validate(&self) -> Result<(), String> {
        if self.team_member_id.is_none()
            && (self.team_member_generation.is_some()
                || self.work_classification != TeamWorkClassification::CoordinationOnly)
        {
            return Err(
                "member-null team run bindings must be coordination_only with no generation"
                    .to_string(),
            );
        }
        Ok(())
    }
    pub fn is_current_member_authority(&self, member_id: &TeamMemberId, generation: i64) -> bool {
        self.team_member_id.as_ref() == Some(member_id)
            && self.team_member_generation == Some(generation)
    }
    pub fn transition_to(
        &mut self,
        next: TeamRunBindingStatus,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        if !self.status.can_transition_to(next) {
            return Err(format!(
                "invalid team run binding transition: {:?} -> {:?}",
                self.status, next
            ));
        }
        self.status = next;
        if next == TeamRunBindingStatus::Launching {
            self.launched_at = Some(now);
        }
        if matches!(
            next,
            TeamRunBindingStatus::Terminal
                | TeamRunBindingStatus::Cancelled
                | TeamRunBindingStatus::Failed
        ) {
            self.terminal_at = Some(now);
        }
        Ok(())
    }
}
