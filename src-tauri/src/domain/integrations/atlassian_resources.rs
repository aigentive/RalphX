//! Atlassian (Jira + Confluence) records, credentials and the outbound
//! Atlassian API port.
//!
//! The HTTP client that implements the port lives in `infrastructure`; the OAuth
//! callback server and orchestration service stay in `application`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::atlassian_api_error::AtlassianApiError;
use super::atlassian_mcp_ops::{
    AtlassianRawMethod, ConfluencePageContent, ConfluencePageCreateRequest,
    ConfluencePageUpdateRequest, JiraIssueCreateRequest, JiraIssueCreated, JiraIssueUpdateRequest,
};
use super::jira_agile_types::{JiraBoardConfiguration, JiraBoardSummary, JiraSprintSummary};
use crate::domain::services::ComposerIntegrationReference;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtlassianResourceKind {
    Jira,
    Confluence,
}

impl std::str::FromStr for AtlassianResourceKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "jira" => Ok(Self::Jira),
            "confluence" => Ok(Self::Confluence),
            other => Err(format!("Unknown Atlassian resource kind: {other}")),
        }
    }
}

impl AtlassianResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jira => "jira",
            Self::Confluence => "confluence",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianResourceSummary {
    pub kind: AtlassianResourceKind,
    pub id: String,
    pub key: Option<String>,
    pub title: String,
    pub url: Option<String>,
    pub excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianResourceUrlResolution {
    pub input_url: String,
    pub resource: Option<AtlassianResourceSummary>,
    /// The exact composer reference kind (for example `"jira_board"` or
    /// `"confluence_link"`) that produced `resource`, so the frontend chip
    /// can render a subtype distinct from the routing-level Jira/Confluence
    /// [`AtlassianResourceKind`] carried on `resource.kind`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianJiraComment {
    pub id: Option<String>,
    pub author: Option<String>,
    pub body_markdown: String,
    pub body_text: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianJiraAttachment {
    pub id: Option<String>,
    pub filename: String,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
    pub author: Option<String>,
    pub content_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub created_at: Option<String>,
}

/// A Jira parent/child issue reference (subtask or epic child), rendered from
/// already-returned issue fields or the capped epic-children lookup — never
/// carries provider URLs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianJiraChildIssue {
    pub key: String,
    pub summary: String,
    pub status: Option<String>,
    pub issue_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianJiraTransition {
    pub provider_transition_id: String,
    pub to_state_id: String,
    pub name: String,
    pub category: String,
}

/// A page of Jira issue comments plus the provider's true total, so callers
/// that only see the newest few comments can point agents at
/// `jira_list_comments` for the rest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraCommentsPage {
    pub comments: Vec<AtlassianJiraComment>,
    pub total: usize,
}

/// A Confluence space (v2 API), used to unblock `confluence_create_page`'s
/// otherwise-unguessable `spaceId`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfluenceSpaceSummary {
    pub id: String,
    pub key: String,
    pub name: String,
}

/// A Jira user match from the bounded account search, used to resolve an
/// `accountId` for `jira_assign_issue`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraUserSummary {
    pub account_id: String,
    pub display_name: String,
}

/// Lightweight summary of a Jira project used as a ticketing container.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraProjectSummary {
    pub id: String,
    pub key: String,
    pub name: String,
}

/// A Jira status (deduped across issue types) with a normalized category, used to
/// build kanban columns for a selected project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraStatusSummary {
    pub id: String,
    pub name: String,
    pub category: String,
}

/// Project-scoped Jira issue detail preserving the status/assignee/labels needed
/// to render kanban columns and the ticket list (richer than the lossy
/// search-summary shape, which only keeps id/key/title).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraIssueDetail {
    pub key: String,
    pub title: String,
    pub status_id: Option<String>,
    pub status_name: Option<String>,
    pub status_category: Option<String>,
    pub assignee_name: Option<String>,
    pub assignee_avatar: Option<String>,
    pub labels: Vec<String>,
    pub updated: Option<String>,
    pub priority: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianResourceContent {
    pub kind: AtlassianResourceKind,
    pub id: String,
    pub key: Option<String>,
    pub title: String,
    pub url: Option<String>,
    pub body: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub reporter: Option<String>,
    #[serde(default)]
    pub updated_at_remote: Option<String>,
    #[serde(default)]
    pub description_markdown: Option<String>,
    #[serde(default)]
    pub description_text: Option<String>,
    #[serde(default)]
    pub acceptance_criteria_markdown: Option<String>,
    #[serde(default)]
    pub acceptance_criteria_text: Option<String>,
    #[serde(default)]
    pub comments: Vec<AtlassianJiraComment>,
    #[serde(default)]
    pub attachments: Vec<AtlassianJiraAttachment>,
    #[serde(default)]
    pub issue_type: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub parent_key: Option<String>,
    #[serde(default)]
    pub children: Vec<AtlassianJiraChildIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianConnectivity {
    pub jira_available: bool,
    pub confluence_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtlassianCredential {
    ApiToken {
        email: String,
        token: String,
    },
    OAuth {
        access_token: String,
        cloud_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlassianAuthContext {
    pub site_url: String,
    pub credential: AtlassianCredential,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianOAuthAuthorization {
    pub authorization_url: String,
    pub state: String,
    pub scopes: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianOAuthTokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianOAuthResource {
    pub id: String,
    pub url: String,
    pub scopes: Vec<String>,
}

#[async_trait]
pub trait AtlassianApiClient: Send + Sync {
    async fn validate(&self, auth: &AtlassianAuthContext) -> Result<AtlassianConnectivity, String>;

    async fn search(
        &self,
        auth: &AtlassianAuthContext,
        kind: AtlassianResourceKind,
        query: &str,
        limit: usize,
    ) -> Result<Vec<AtlassianResourceSummary>, String>;

    async fn fetch(
        &self,
        auth: &AtlassianAuthContext,
        reference: &ComposerIntegrationReference,
    ) -> Result<AtlassianResourceContent, String>;

    /// Search using the caller's query string verbatim (raw JQL/CQL), with no
    /// smart-mode rewriting. Errors preserve the Atlassian HTTP status so
    /// callers can surface it (for example malformed JQL) instead of a flat
    /// string.
    async fn search_raw(
        &self,
        _auth: &AtlassianAuthContext,
        _kind: AtlassianResourceKind,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<AtlassianResourceSummary>, AtlassianApiError> {
        Err(AtlassianApiError::transport(
            "Raw Atlassian query pass-through is not available for this client",
        ))
    }

    async fn assign_jira_issue_to_current_user(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
    ) -> Result<(), String>;

    async fn clear_jira_issue_assignee(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
    ) -> Result<(), String> {
        Err("Jira issue assignee clearing is not available for this client".to_string())
    }

    /// Assigns a Jira issue to a specific account. Takes precedence over
    /// `assign_to_me` in `jira_assign_issue`'s resolution order.
    async fn assign_jira_issue_to_account(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
        _account_id: &str,
    ) -> Result<(), String> {
        Err("Jira issue assignment by account is not available for this client".to_string())
    }

    async fn list_jira_issue_transitions(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
    ) -> Result<Vec<AtlassianJiraTransition>, String> {
        Err("Jira workflow transitions are not available for this client".to_string())
    }

    async fn transition_jira_issue(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
        _transition_id: &str,
    ) -> Result<(), String> {
        Err("Jira issue transitions are not available for this client".to_string())
    }

    async fn add_jira_comment(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
        _body_markdown: &str,
    ) -> Result<AtlassianJiraComment, String> {
        Err("Jira comments are not available for this client".to_string())
    }

    async fn set_jira_issue_labels(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
        _labels: Vec<String>,
    ) -> Result<(), String> {
        Err("Jira label writes are not available for this client".to_string())
    }

    async fn list_jira_projects(
        &self,
        _auth: &AtlassianAuthContext,
        _limit: usize,
    ) -> Result<Vec<JiraProjectSummary>, String> {
        Err("Jira project enumeration is not available for this client".to_string())
    }

    async fn list_jira_project_statuses(
        &self,
        _auth: &AtlassianAuthContext,
        _project_key: &str,
    ) -> Result<Vec<JiraStatusSummary>, String> {
        Err("Jira project statuses are not available for this client".to_string())
    }

    async fn list_jira_project_issues(
        &self,
        _auth: &AtlassianAuthContext,
        _project_key: &str,
        _limit: usize,
    ) -> Result<Vec<JiraIssueDetail>, String> {
        Err("Jira project issues are not available for this client".to_string())
    }

    async fn list_jira_boards(
        &self,
        _auth: &AtlassianAuthContext,
        _project_key: &str,
    ) -> Result<Vec<JiraBoardSummary>, String> {
        Err("Jira board enumeration is not available for this client".to_string())
    }

    async fn get_jira_board_configuration(
        &self,
        _auth: &AtlassianAuthContext,
        _board_id: &str,
    ) -> Result<JiraBoardConfiguration, String> {
        Err("Jira board configuration is not available for this client".to_string())
    }

    async fn list_jira_active_sprints(
        &self,
        _auth: &AtlassianAuthContext,
        _board_id: &str,
    ) -> Result<Vec<JiraSprintSummary>, String> {
        Err("Jira sprint enumeration is not available for this client".to_string())
    }

    /// Lists issues in a Jira Software sprint as enriched summaries (status,
    /// issue type, assignee, updated timestamp), capped at `limit`.
    async fn list_jira_sprint_issues(
        &self,
        _auth: &AtlassianAuthContext,
        _sprint_id: &str,
        _limit: usize,
    ) -> Result<Vec<AtlassianResourceSummary>, String> {
        Err("Jira sprint issue enumeration is not available for this client".to_string())
    }

    /// Lists comments on a Jira issue with the provider's true total, capped
    /// at `max_results` starting from `start_at`.
    async fn list_jira_comments(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
        _start_at: usize,
        _max_results: usize,
    ) -> Result<JiraCommentsPage, String> {
        Err("Jira comment listing is not available for this client".to_string())
    }

    /// Lists Confluence spaces (v2 API) visible to the connected user, capped
    /// at `limit`.
    async fn list_confluence_spaces(
        &self,
        _auth: &AtlassianAuthContext,
        _limit: usize,
    ) -> Result<Vec<ConfluenceSpaceSummary>, String> {
        Err("Confluence space enumeration is not available for this client".to_string())
    }

    /// Bounded Jira user search (max 20 results), used to resolve an
    /// `accountId` for `jira_assign_issue`.
    async fn search_jira_users(
        &self,
        _auth: &AtlassianAuthContext,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<JiraUserSummary>, String> {
        Err("Jira user search is not available for this client".to_string())
    }

    // ---- Atlassian MCP tool operations ---------------------------------
    //
    // These return `AtlassianApiError` so callers classify failures on the
    // numeric HTTP status instead of parsing message text.

    async fn create_jira_issue(
        &self,
        _auth: &AtlassianAuthContext,
        _request: &JiraIssueCreateRequest,
    ) -> Result<JiraIssueCreated, AtlassianApiError> {
        Err(AtlassianApiError::transport(
            "Jira issue creation is not available for this client",
        ))
    }

    async fn update_jira_issue(
        &self,
        _auth: &AtlassianAuthContext,
        _issue_key: &str,
        _request: &JiraIssueUpdateRequest,
    ) -> Result<(), AtlassianApiError> {
        Err(AtlassianApiError::transport(
            "Jira issue updates are not available for this client",
        ))
    }

    async fn confluence_get_page(
        &self,
        _auth: &AtlassianAuthContext,
        _page_id: &str,
    ) -> Result<ConfluencePageContent, AtlassianApiError> {
        Err(AtlassianApiError::transport(
            "Confluence page reads are not available for this client",
        ))
    }

    async fn confluence_create_page(
        &self,
        _auth: &AtlassianAuthContext,
        _request: &ConfluencePageCreateRequest,
    ) -> Result<ConfluencePageContent, AtlassianApiError> {
        Err(AtlassianApiError::transport(
            "Confluence page creation is not available for this client",
        ))
    }

    async fn confluence_update_page(
        &self,
        _auth: &AtlassianAuthContext,
        _page_id: &str,
        _request: &ConfluencePageUpdateRequest,
    ) -> Result<ConfluencePageContent, AtlassianApiError> {
        Err(AtlassianApiError::transport(
            "Confluence page updates are not available for this client",
        ))
    }

    async fn raw_api_request(
        &self,
        _auth: &AtlassianAuthContext,
        _method: AtlassianRawMethod,
        _kind: AtlassianResourceKind,
        _path: &str,
        _body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, AtlassianApiError> {
        Err(AtlassianApiError::transport(
            "Generic Atlassian API requests are not available for this client",
        ))
    }

    async fn exchange_oauth_code(
        &self,
        client_id: &str,
        client_secret: &str,
        code: &str,
        redirect_uri: &str,
    ) -> Result<AtlassianOAuthTokenResponse, String>;

    async fn refresh_oauth_token(
        &self,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<AtlassianOAuthTokenResponse, String>;

    async fn oauth_accessible_resources(
        &self,
        access_token: &str,
    ) -> Result<Vec<AtlassianOAuthResource>, String>;
}
