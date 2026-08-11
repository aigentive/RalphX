//! Linear ticketing-provider settings, wire records, and outbound ports.
//!
//! Follows the `clickup_settings` / `granola_settings` pattern: the settings
//! record, its repository port, the issue/comment/label records exchanged with
//! Linear, and the `LinearApiClient` port live in the domain. The HTTP client
//! that implements the port lives in `infrastructure`, and the orchestration
//! service that consumes it lives in `application`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::IntegrationValidationStatus;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearIntegrationSettings {
    pub enabled: bool,
    pub token_secret_ref: Option<String>,
    pub validation_status: IntegrationValidationStatus,
    pub issue_search_available: bool,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl Default for LinearIntegrationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            token_secret_ref: None,
            validation_status: IntegrationValidationStatus::NotConfigured,
            issue_search_available: false,
            last_validated_at: None,
            last_error: None,
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearAuthContext {
    pub api_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearIssueSummary {
    pub id: String,
    pub key: Option<String>,
    pub title: String,
    pub url: Option<String>,
    pub excerpt: Option<String>,
    pub state_id: Option<String>,
    pub state_name: Option<String>,
    pub state_category: Option<String>,
    pub state_color: Option<String>,
    pub assignee: Option<String>,
    pub updated_at: Option<String>,
    pub labels: Vec<String>,
    pub project: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearIssueContent {
    pub id: String,
    pub key: Option<String>,
    pub title: String,
    pub url: Option<String>,
    pub body: String,
    pub state_name: Option<String>,
    pub assignee: Option<String>,
    pub creator: Option<String>,
    pub updated_at: Option<String>,
    pub comments: Vec<LinearComment>,
    pub attachments: Vec<LinearAttachment>,
    pub labels: Vec<String>,
    pub project: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearAttachment {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearWorkflowState {
    pub id: String,
    pub name: String,
    pub category: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearProject {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearLabel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearUser {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearComment {
    pub id: String,
    pub body: String,
    pub author_id: Option<String>,
    pub author_name: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[async_trait]
pub trait LinearIntegrationSettingsRepository: Send + Sync {
    async fn get(&self) -> Result<LinearIntegrationSettings, Box<dyn std::error::Error>>;

    async fn upsert(
        &self,
        settings: &LinearIntegrationSettings,
    ) -> Result<LinearIntegrationSettings, Box<dyn std::error::Error>>;
}

#[async_trait]
pub trait LinearApiClient: Send + Sync {
    async fn validate(&self, auth: &LinearAuthContext) -> Result<(), String>;

    async fn search_issues(
        &self,
        auth: &LinearAuthContext,
        query: &str,
        limit: usize,
    ) -> Result<Vec<LinearIssueSummary>, String>;

    async fn fetch_issue(
        &self,
        auth: &LinearAuthContext,
        reference: &crate::domain::services::ComposerIntegrationReference,
    ) -> Result<LinearIssueContent, String>;

    async fn list_workflow_states(
        &self,
        _auth: &LinearAuthContext,
        _team_id: Option<&str>,
    ) -> Result<Vec<LinearWorkflowState>, String> {
        Err("Linear workflow states are not available for this client".to_string())
    }

    async fn current_user(&self, _auth: &LinearAuthContext) -> Result<LinearUser, String> {
        Err("Linear current-user lookup is not available for this client".to_string())
    }

    async fn update_issue_state(
        &self,
        _auth: &LinearAuthContext,
        _issue_id: &str,
        _state_id: &str,
    ) -> Result<(), String> {
        Err("Linear issue state updates are not available for this client".to_string())
    }

    async fn assign_issue_to_current_user(
        &self,
        _auth: &LinearAuthContext,
        _issue_id: &str,
    ) -> Result<LinearUser, String> {
        Err("Linear issue assignment is not available for this client".to_string())
    }

    async fn clear_issue_assignee(
        &self,
        _auth: &LinearAuthContext,
        _issue_id: &str,
    ) -> Result<(), String> {
        Err("Linear issue assignee clearing is not available for this client".to_string())
    }

    async fn create_comment(
        &self,
        _auth: &LinearAuthContext,
        _issue_id: &str,
        _body_markdown: &str,
    ) -> Result<LinearComment, String> {
        Err("Linear comments are not available for this client".to_string())
    }

    async fn list_projects(
        &self,
        _auth: &LinearAuthContext,
        _first: usize,
    ) -> Result<Vec<LinearProject>, String> {
        Err("Linear projects are not available for this client".to_string())
    }

    async fn list_issue_team_labels(
        &self,
        _auth: &LinearAuthContext,
        _issue_id: &str,
    ) -> Result<Vec<LinearLabel>, String> {
        Err("Linear issue labels are not available for this client".to_string())
    }

    async fn update_issue_labels(
        &self,
        _auth: &LinearAuthContext,
        _issue_id: &str,
        _label_ids: Vec<String>,
    ) -> Result<(), String> {
        Err("Linear issue label updates are not available for this client".to_string())
    }
}
