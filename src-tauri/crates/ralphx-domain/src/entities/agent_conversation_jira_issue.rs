use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::entities::{ChatConversationId, ChatMessageId, ProjectId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationJiraRefreshStatus {
    NotLoaded,
    Loaded,
    Error,
}

impl std::fmt::Display for AgentConversationJiraRefreshStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotLoaded => write!(f, "not_loaded"),
            Self::Loaded => write!(f, "loaded"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl FromStr for AgentConversationJiraRefreshStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "not_loaded" => Ok(Self::NotLoaded),
            "loaded" => Ok(Self::Loaded),
            "error" => Ok(Self::Error),
            _ => Err(format!("unknown Jira refresh status: '{value}'")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversationJiraIssueLink {
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub provider: String,
    pub issue_key: String,
    pub issue_id: Option<String>,
    pub issue_url: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub reporter: Option<String>,
    pub updated_at_remote: Option<String>,
    pub description_markdown: Option<String>,
    pub description_text: Option<String>,
    pub acceptance_criteria_markdown: Option<String>,
    pub acceptance_criteria_text: Option<String>,
    pub comments_json: String,
    pub attachments_json: String,
    pub last_refreshed_at: Option<DateTime<Utc>>,
    pub refresh_status: AgentConversationJiraRefreshStatus,
    pub refresh_error: Option<String>,
    pub assigned_at: DateTime<Utc>,
    pub assigned_from_message_id: Option<ChatMessageId>,
    pub manually_assigned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentConversationJiraIssueLink {
    pub fn new(
        conversation_id: ChatConversationId,
        project_id: ProjectId,
        issue_key: String,
        assigned_at: DateTime<Utc>,
    ) -> Self {
        Self {
            conversation_id,
            project_id,
            provider: "atlassian".to_string(),
            issue_key,
            issue_id: None,
            issue_url: None,
            title: None,
            status: None,
            assignee: None,
            reporter: None,
            updated_at_remote: None,
            description_markdown: None,
            description_text: None,
            acceptance_criteria_markdown: None,
            acceptance_criteria_text: None,
            comments_json: "[]".to_string(),
            attachments_json: "[]".to_string(),
            last_refreshed_at: None,
            refresh_status: AgentConversationJiraRefreshStatus::NotLoaded,
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
        issue_id: Option<String>,
        title: Option<String>,
        issue_url: Option<String>,
    ) -> Self {
        self.issue_id = issue_id;
        self.title = title;
        self.issue_url = issue_url;
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
#[path = "agent_conversation_jira_issue_tests.rs"]
mod tests;
