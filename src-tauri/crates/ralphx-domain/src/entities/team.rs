// Domain entities for team history persistence
// Maps to team_sessions and team_messages tables (v37 migration)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// Conversation coordination mode projected to clients.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationMode {
    #[default]
    Solo,
    LegacyClaudeTeam,
    RxNativeTeam,
    RxNativeWorkflow,
    CodexNativeUltra,
}

impl fmt::Display for CoordinationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Solo => write!(f, "solo"),
            Self::LegacyClaudeTeam => write!(f, "legacy_claude_team"),
            Self::RxNativeTeam => write!(f, "rx_native_team"),
            Self::RxNativeWorkflow => write!(f, "rx_native_workflow"),
            Self::CodexNativeUltra => write!(f, "codex_native_ultra"),
        }
    }
}

impl FromStr for CoordinationMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "solo" => Ok(Self::Solo),
            "legacy_claude_team" => Ok(Self::LegacyClaudeTeam),
            "rx_native_team" => Ok(Self::RxNativeTeam),
            "rx_native_workflow" => Ok(Self::RxNativeWorkflow),
            "codex_native_ultra" => Ok(Self::CodexNativeUltra),
            other => Err(format!(
                "Invalid coordination mode '{}'. Valid values: solo, legacy_claude_team, rx_native_team, rx_native_workflow, codex_native_ultra",
                other
            )),
        }
    }
}

/// Optional strategy hint for a native team request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamIntentStrategy {
    Research,
    Debate,
    Execution,
}

/// Explicit native team-mode request supplied by the UI or compatibility bridge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamIntent {
    pub coordination_mode: CoordinationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<TeamIntentStrategy>,
}

/// Provider-neutral capability request. `TeamIntent` remains the compatibility
/// name on existing transport surfaces while callers migrate.
pub type CapabilityIntent = TeamIntent;

impl TeamIntent {
    pub fn rx_native(strategy: Option<TeamIntentStrategy>) -> Self {
        Self {
            coordination_mode: CoordinationMode::RxNativeTeam,
            strategy,
        }
    }

    pub fn is_solo(&self) -> bool {
        self.coordination_mode == CoordinationMode::Solo
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageTargetKind {
    Coordinator,
    Member,
    Broadcast,
}

/// Native mailbox target for team messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMessageTarget {
    pub kind: TeamMessageTargetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_member_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}

/// Unique identifier for a TeamSession
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamSessionId(pub String);

impl TeamSessionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
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
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a TeamMessage
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamMessageId(pub String);

impl TeamMessageId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
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

impl std::fmt::Display for TeamMessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Aggregated token and dollar cost for a teammate session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeammateCost {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub estimated_usd: f64,
}

/// Snapshot of a teammate's state at a point in time
/// Stored as JSON in team_sessions.teammate_json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateSnapshot {
    pub name: String,
    pub color: String,
    pub model: String,
    pub role: String,
    pub status: String,
    pub cost: TeammateCost,
    pub spawned_at: String,
    pub last_activity_at: String,
    /// Conversation ID linking to this teammate's chat_conversations row.
    /// Added after v37 — `#[serde(default)]` ensures existing JSON blobs
    /// without this field deserialize as None.
    #[serde(default)]
    pub conversation_id: Option<String>,
}

/// A team session — one row per active/historical team
/// Maps to team_sessions table (v37 migration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSession {
    pub id: TeamSessionId,
    pub team_name: String,
    pub context_id: String,
    pub context_type: String,
    pub lead_name: Option<String>,
    pub phase: String,
    pub teammates: Vec<TeammateSnapshot>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub disbanded_at: Option<DateTime<Utc>>,
}

impl TeamSession {
    pub fn new(
        team_name: impl Into<String>,
        context_id: impl Into<String>,
        context_type: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: TeamSessionId::new(),
            team_name: team_name.into(),
            context_id: context_id.into(),
            context_type: context_type.into(),
            lead_name: None,
            phase: "forming".to_string(),
            teammates: Vec::new(),
            created_at: now,
            updated_at: now,
            disbanded_at: None,
        }
    }
}

/// A single message in a team session
/// Maps to team_messages table (v37 migration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMessageRecord {
    pub id: TeamMessageId,
    pub team_session_id: TeamSessionId,
    pub sender: String,
    pub recipient: Option<String>,
    pub content: String,
    pub message_type: String,
    pub created_at: DateTime<Utc>,
}

impl TeamMessageRecord {
    pub fn new(
        team_session_id: TeamSessionId,
        sender: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: TeamMessageId::new(),
            team_session_id,
            sender: sender.into(),
            recipient: None,
            content: content.into(),
            message_type: "teammate_message".to_string(),
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
#[path = "team_tests.rs"]
mod tests;
