use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::agents::AgentHarnessKind;

use super::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DelegatedSessionId(pub String);

impl DelegatedSessionId {
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

impl Default for DelegatedSessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for DelegatedSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegatedSession {
    pub id: DelegatedSessionId,
    pub project_id: ProjectId,
    pub parent_context_type: String,
    pub parent_context_id: String,
    pub parent_turn_id: Option<String>,
    pub parent_message_id: Option<String>,
    pub delegate_context_authorized: bool,
    pub caller_conversation_id: Option<String>,
    /// Native delegation job identity; absent for sessions created before this field existed.
    pub job_id: Option<String>,
    /// Trusted caller run identity; absent for legacy or externally created sessions.
    pub parent_agent_run_id: Option<String>,
    pub agent_name: String,
    pub title: Option<String>,
    pub harness: AgentHarnessKind,
    pub status: String,
    pub provider_session_id: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl DelegatedSession {
    pub fn new(
        project_id: ProjectId,
        parent_context_type: impl Into<String>,
        parent_context_id: impl Into<String>,
        agent_name: impl Into<String>,
        harness: AgentHarnessKind,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: DelegatedSessionId::new(),
            project_id,
            parent_context_type: parent_context_type.into(),
            parent_context_id: parent_context_id.into(),
            parent_turn_id: None,
            parent_message_id: None,
            delegate_context_authorized: true,
            caller_conversation_id: None,
            job_id: None,
            parent_agent_run_id: None,
            agent_name: agent_name.into(),
            title: None,
            harness,
            status: "running".to_string(),
            provider_session_id: None,
            error: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }
}
