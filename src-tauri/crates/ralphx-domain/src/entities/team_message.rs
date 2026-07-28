use super::{AgentRunId, AgentTaskAssignmentId, TeamMemberId, TeamSessionId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamMessageId(pub String);
impl TeamMessageId {
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
impl Default for TeamMessageId {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageActorKind {
    Coordinator,
    Member,
    System,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageKind {
    Instruction,
    Result,
    Question,
    Status,
    Control,
    Approval,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageTarget {
    Coordinator,
    Member,
    Broadcast,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMessage {
    pub id: TeamMessageId,
    pub team_id: TeamSessionId,
    pub sequence: i64,
    pub sender_kind: TeamMessageActorKind,
    pub sender_member_id: Option<TeamMemberId>,
    pub target_kind: TeamMessageTarget,
    pub target_member_id: Option<TeamMemberId>,
    pub kind: TeamMessageKind,
    pub content: String,
    pub source_run_id: Option<AgentRunId>,
    pub assignment_id: Option<AgentTaskAssignmentId>,
    pub idempotency_key: String,
    pub created_at: DateTime<Utc>,
}
impl TeamMessage {
    pub fn validate(&self) -> Result<(), String> {
        if self.sequence < 1
            || self.content.is_empty()
            || self.content.len() > 32_000
            || self.idempotency_key.trim().is_empty()
        {
            return Err(
                "team message has invalid sequence, content, or idempotency key".to_string(),
            );
        }
        if matches!(self.target_kind, TeamMessageTarget::Member) != self.target_member_id.is_some()
        {
            return Err("member target must carry exactly one member id".to_string());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamMessageDeliveryId(pub String);
impl TeamMessageDeliveryId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}
impl Default for TeamMessageDeliveryId {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageDeliveryStatus {
    Pending,
    Queued,
    Delivered,
    Acknowledged,
    Cancelled,
    Failed,
}
impl TeamMessageDeliveryStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Queued | Self::Cancelled | Self::Failed)
                | (
                    Self::Queued,
                    Self::Delivered | Self::Cancelled | Self::Failed
                )
                | (
                    Self::Delivered,
                    Self::Acknowledged | Self::Cancelled | Self::Failed
                )
                | (Self::Failed, Self::Pending | Self::Queued | Self::Cancelled)
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMessageDelivery {
    pub id: TeamMessageDeliveryId,
    pub message_id: TeamMessageId,
    pub recipient_kind: TeamMessageActorKind,
    pub recipient_member_id: Option<TeamMemberId>,
    pub recipient_generation: Option<i64>,
    pub status: TeamMessageDeliveryStatus,
    pub queued_message_id: Option<String>,
    pub attempt_count: i64,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
impl TeamMessageDelivery {
    pub fn transition_to(
        &mut self,
        next: TeamMessageDeliveryStatus,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        if !self.status.can_transition_to(next) {
            return Err(format!(
                "invalid team delivery transition: {:?} -> {:?}",
                self.status, next
            ));
        }
        self.status = next;
        self.updated_at = now;
        Ok(())
    }
}
