use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{ChatConversationId, ProjectId, TeamIntentStrategy};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamSessionId(pub String);

impl TeamSessionId {
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
impl Default for TeamSessionId {
    fn default() -> Self {
        Self::new()
    }
}
impl std::fmt::Display for TeamSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamSessionStatus {
    Active,
    Suspending,
    Suspended,
    Draining,
    Closed,
    Failed,
}

impl TeamSessionStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Active, Self::Suspending | Self::Draining)
                | (Self::Suspending, Self::Suspended)
                | (Self::Suspended, Self::Active | Self::Draining)
                | (Self::Draining, Self::Closed)
                | (Self::Failed, Self::Suspended | Self::Draining)
        )
    }
    pub fn transition(self, next: Self) -> Result<Self, String> {
        self.can_transition_to(next)
            .then_some(next)
            .ok_or_else(|| format!("invalid team session transition: {self:?} -> {next:?}"))
    }
    pub fn is_closed(self) -> bool {
        self == Self::Closed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamBudgetPolicy {
    pub token_limit: Option<u64>,
    pub cost_limit_micros: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamSession {
    pub id: TeamSessionId,
    pub project_id: ProjectId,
    pub coordinator_conversation_id: ChatConversationId,
    pub status: TeamSessionStatus,
    pub strategy: Option<TeamIntentStrategy>,
    pub configured_concurrency: u32,
    pub effective_concurrency: u32,
    pub automatic_wake_limit: u32,
    pub budget_policy: Option<TeamBudgetPolicy>,
    pub pending_coordination_mode: Option<String>,
    pub pending_exit_action: Option<String>,
    pub version: i64,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

impl TeamSession {
    pub fn transition_to(
        &mut self,
        next: TeamSessionStatus,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.status = self.status.transition(next)?;
        self.updated_at = now;
        if next == TeamSessionStatus::Closed {
            self.closed_at = Some(now);
        }
        Ok(())
    }
}
