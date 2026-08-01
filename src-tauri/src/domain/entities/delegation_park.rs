use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{AgentRunId, ChatConversationId};

/// Unique identifier for a durable delegation park record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DelegationParkId(Uuid);

impl DelegationParkId {
    /// Create a new random delegation park ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create an ID from persisted storage.
    pub fn from_string(value: impl Into<String>) -> Self {
        let value = value.into();
        Self(Uuid::parse_str(&value).unwrap_or_else(|_| Uuid::nil()))
    }

    /// Return the storage representation of this ID.
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    /// Return the underlying UUID.
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for DelegationParkId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DelegationParkId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl From<Uuid> for DelegationParkId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<DelegationParkId> for String {
    fn from(id: DelegationParkId) -> Self {
        id.0.to_string()
    }
}

impl FromStr for DelegationParkId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(value)?))
    }
}

/// Durable lifecycle state for a delegation park.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationParkState {
    Armed,
    Waking,
    Woken,
    Superseded,
    Expired,
    Failed,
}

impl DelegationParkState {
    /// Return the SQLite storage representation for this state.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Armed => "armed",
            Self::Waking => "waking",
            Self::Woken => "woken",
            Self::Superseded => "superseded",
            Self::Expired => "expired",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for DelegationParkState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DelegationParkState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "armed" => Ok(Self::Armed),
            "waking" => Ok(Self::Waking),
            "woken" => Ok(Self::Woken),
            "superseded" => Ok(Self::Superseded),
            "expired" => Ok(Self::Expired),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("Invalid delegation park state: {value}")),
        }
    }
}

/// Policy that decides which settled delegate jobs wake a parked parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationWakePolicy {
    AllSettled,
    AnySettled,
}

impl DelegationWakePolicy {
    /// Return the SQLite storage representation for this policy.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllSettled => "all_settled",
            Self::AnySettled => "any_settled",
        }
    }
}

impl fmt::Display for DelegationWakePolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DelegationWakePolicy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "all_settled" => Ok(Self::AllSettled),
            "any_settled" => Ok(Self::AnySettled),
            _ => Err(format!("Invalid delegation wake policy: {value}")),
        }
    }
}

/// Wake decision shared by live delegate settlement and startup recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationWakeDecision {
    Wait,
    Wake(DelegationWakeReason),
}

/// Reason that a parked parent should receive a hidden resume message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationWakeReason {
    AllSettled,
    AnySettled,
    DelegateFailed,
    Deadline,
}

/// One delegate job tracked by a durable delegation park.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationParkJob {
    pub job_id: String,
    pub delegated_session_id: String,
    pub delegated_agent_run_id: AgentRunId,
    pub settled_status: Option<String>,
}

/// Durable parent-turn suspension that wakes when tracked delegate jobs settle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationPark {
    pub id: DelegationParkId,
    pub parent_conversation_id: ChatConversationId,
    pub parent_agent_run_id: AgentRunId,
    pub generation: i64,
    pub wake_policy: DelegationWakePolicy,
    pub wake_on_failure: bool,
    pub state: DelegationParkState,
    pub deadline_at: DateTime<Utc>,
    pub wake_attempts: i32,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub jobs: Vec<DelegationParkJob>,
}

impl DelegationPark {
    /// Decide whether settled delegate jobs should wake the parked parent.
    pub fn wake_decision(&self) -> DelegationWakeDecision {
        if self.wake_on_failure
            && self
                .jobs
                .iter()
                .any(|job| matches!(job.settled_status.as_deref(), Some("failed" | "cancelled")))
        {
            return DelegationWakeDecision::Wake(DelegationWakeReason::DelegateFailed);
        }

        match self.wake_policy {
            DelegationWakePolicy::AllSettled
                if !self.jobs.is_empty()
                    && self.jobs.iter().all(|job| job.settled_status.is_some()) =>
            {
                DelegationWakeDecision::Wake(DelegationWakeReason::AllSettled)
            }
            DelegationWakePolicy::AnySettled
                if self.jobs.iter().any(|job| job.settled_status.is_some()) =>
            {
                DelegationWakeDecision::Wake(DelegationWakeReason::AnySettled)
            }
            _ => DelegationWakeDecision::Wait,
        }
    }

    /// Return whether the park's deadline has passed at `now`.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.deadline_at
    }
}
