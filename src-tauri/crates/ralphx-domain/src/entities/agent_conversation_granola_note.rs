use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::entities::{ChatConversationId, ChatMessageId, ProjectId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationGranolaRefreshStatus {
    NotLoaded,
    Loaded,
    Error,
}

impl std::fmt::Display for AgentConversationGranolaRefreshStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotLoaded => write!(f, "not_loaded"),
            Self::Loaded => write!(f, "loaded"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl FromStr for AgentConversationGranolaRefreshStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "not_loaded" => Ok(Self::NotLoaded),
            "loaded" => Ok(Self::Loaded),
            "error" => Ok(Self::Error),
            _ => Err(format!("unknown Granola refresh status: '{value}'")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversationGranolaNoteLink {
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub provider: String,
    pub note_id: String,
    pub note_url: Option<String>,
    pub title: Option<String>,
    pub summary_markdown: Option<String>,
    pub transcript_json: String,
    pub include_transcript: bool,
    pub last_refreshed_at: Option<DateTime<Utc>>,
    pub refresh_status: AgentConversationGranolaRefreshStatus,
    pub refresh_error: Option<String>,
    pub assigned_at: DateTime<Utc>,
    pub assigned_from_message_id: Option<ChatMessageId>,
    pub manually_assigned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentConversationGranolaNoteLink {
    pub fn new(
        conversation_id: ChatConversationId,
        project_id: ProjectId,
        note_id: String,
        assigned_at: DateTime<Utc>,
    ) -> Self {
        Self {
            conversation_id,
            project_id,
            provider: "granola".to_string(),
            note_id,
            note_url: None,
            title: None,
            summary_markdown: None,
            transcript_json: "[]".to_string(),
            include_transcript: true,
            last_refreshed_at: None,
            refresh_status: AgentConversationGranolaRefreshStatus::NotLoaded,
            refresh_error: None,
            assigned_at,
            assigned_from_message_id: None,
            manually_assigned: false,
            created_at: assigned_at,
            updated_at: assigned_at,
        }
    }

    pub fn with_reference_metadata(
        mut self,
        title: Option<String>,
        note_url: Option<String>,
        summary_markdown: Option<String>,
        include_transcript: bool,
    ) -> Self {
        self.title = title;
        self.note_url = note_url;
        self.summary_markdown = summary_markdown;
        self.include_transcript = include_transcript;
        self
    }

    pub fn with_assignment_source(
        mut self,
        assigned_from_message_id: Option<ChatMessageId>,
        manually_assigned: bool,
    ) -> Self {
        self.assigned_from_message_id = assigned_from_message_id;
        self.manually_assigned = manually_assigned;
        self
    }
}

#[cfg(test)]
#[path = "agent_conversation_granola_note_tests.rs"]
mod tests;
