use super::{AgentRunId, TeamMemberId, TeamMessageDeliveryId, TeamSessionId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamWakeBatchId(pub String);
impl TeamWakeBatchId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}
impl Default for TeamWakeBatchId {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamWakeRecipientKind {
    Coordinator,
    Member,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamWakeBatchStatus {
    Queued,
    Launching,
    Running,
    Settled,
    Cancelled,
    Failed,
}
impl TeamWakeBatchStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Queued,
                Self::Launching | Self::Cancelled | Self::Failed
            ) | (
                Self::Launching,
                Self::Running | Self::Cancelled | Self::Failed
            ) | (
                Self::Running,
                Self::Settled | Self::Cancelled | Self::Failed
            ) | (Self::Failed, Self::Queued | Self::Cancelled)
        )
    }
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Launching | Self::Running)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamWakeBatch {
    pub id: TeamWakeBatchId,
    pub team_id: TeamSessionId,
    pub recipient_kind: TeamWakeRecipientKind,
    pub recipient_member_id: Option<TeamMemberId>,
    pub recipient_generation: Option<i64>,
    pub first_message_sequence: i64,
    pub last_message_sequence: i64,
    pub delivery_ids: Vec<TeamMessageDeliveryId>,
    pub status: TeamWakeBatchStatus,
    pub planned_agent_run_id: Option<AgentRunId>,
    pub bound_agent_run_id: Option<AgentRunId>,
    pub attempt_count: i64,
    pub budget_count: i64,
    pub lease_token: Option<String>,
    pub version: i64,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}
impl TeamWakeBatch {
    pub fn validate(&self, max_retries: i64) -> Result<(), String> {
        if self.first_message_sequence < 1
            || self.first_message_sequence > self.last_message_sequence
            || self.delivery_ids.is_empty()
            || self.attempt_count > max_retries
        {
            return Err(
                "invalid team wake batch sequence, deliveries, or retry budget".to_string(),
            );
        }
        if matches!(self.recipient_kind, TeamWakeRecipientKind::Member)
            != self.recipient_member_id.is_some()
        {
            return Err("member wake batch must carry exactly one member id".to_string());
        }
        Ok(())
    }
    pub fn transition_to(
        &mut self,
        next: TeamWakeBatchStatus,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        if !self.status.can_transition_to(next) {
            return Err(format!(
                "invalid team wake transition: {:?} -> {:?}",
                self.status, next
            ));
        }
        self.status = next;
        self.updated_at = now;
        if matches!(
            next,
            TeamWakeBatchStatus::Settled | TeamWakeBatchStatus::Cancelled
        ) {
            self.settled_at = Some(now)
        }
        Ok(())
    }
}
