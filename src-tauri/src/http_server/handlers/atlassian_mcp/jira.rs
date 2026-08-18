//! Jira endpoints for the Atlassian MCP tool surface.
//!
//! Schemas never accept run, conversation, or orchestration ids: caller
//! identity is transport-owned and read from headers.

use axum::{extract::State, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};

use crate::application::{
    AtlassianResourceKind, JiraIssueCreateRequest, JiraIssueCreated, JiraIssueUpdateRequest,
    SearchMode,
};
use crate::domain::agents::AtlassianMcpAccess;
use crate::http_server::HttpServerState;

use super::{authorize, required_field, AtlassianMcpHttpError};

/// Upper bound on Jira search results, mirroring the MCP tool schema.
const MAX_SEARCH_RESULTS: usize = 50;

/// Upper bound on `jira_list_comments` results per page.
const MAX_COMMENT_RESULTS: usize = 100;

/// Upper bound on `jira_search_users` results, mirroring the MCP tool schema.
const MAX_USER_SEARCH_RESULTS: usize = 20;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraSearchRequest {
    pub query: String,
    #[serde(default)]
    pub max_results: Option<usize>,
    /// Opt in to raw JQL pass-through: `query` is submitted to Jira verbatim
    /// instead of being rewritten into an issue-key/phrase search.
    #[serde(default)]
    pub jql: bool,
}

#[derive(Debug, Serialize)]
pub struct JiraSearchResponse {
    pub issues: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraIssueKeyRequest {
    pub issue_key: String,
}

#[derive(Debug, Serialize)]
pub struct JiraIssueResponse {
    pub issue: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraListProjectsRequest {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct JiraProjectsResponse {
    pub projects: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct JiraTransitionsResponse {
    pub transitions: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraCreateIssueRequest {
    pub project_key: String,
    pub issue_type: String,
    pub summary: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub priority: Option<String>,
    /// Epic/parent link (maps to `fields.parent`).
    #[serde(default)]
    pub parent_key: Option<String>,
    #[serde(default)]
    pub assignee_account_id: Option<String>,
    #[serde(default)]
    pub components: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct JiraCreateIssueResponse {
    pub issue: JiraIssueCreated,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraUpdateIssueRequest {
    pub issue_key: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub priority: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraAddCommentRequest {
    pub issue_key: String,
    pub body: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraTransitionIssueRequest {
    pub issue_key: String,
    pub transition_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraAssignIssueRequest {
    pub issue_key: String,
    /// When false (and `accountId` is absent), the current assignee is
    /// cleared instead of set.
    #[serde(default)]
    pub assign_to_me: bool,
    /// Explicit assignee. Takes precedence over `assignToMe` when present.
    #[serde(default)]
    pub account_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JiraAckResponse {
    pub ok: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraListCommentsRequest {
    pub issue_key: String,
    #[serde(default)]
    pub start_at: Option<usize>,
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraListCommentsResponse {
    pub comments: Vec<serde_json::Value>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraSearchUsersRequest {
    pub query: String,
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct JiraSearchUsersResponse {
    pub users: Vec<serde_json::Value>,
}

pub async fn jira_search_issues(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraSearchRequest>,
) -> Result<Json<JiraSearchResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::Read).await?;
    let query = required_field(&request.query, "Jira search query is required")?;
    let limit = request
        .max_results
        .unwrap_or(25)
        .clamp(1, MAX_SEARCH_RESULTS);
    let mode = if request.jql {
        SearchMode::RawQuery
    } else {
        SearchMode::Smart
    };

    let results = state
        .app_state
        .atlassian_integration_service
        .search_resources_with_mode(AtlassianResourceKind::Jira, &query, limit, mode)
        .await
        .map_err(|error| match mode {
            // Raw JQL is caller-authored: a Jira 400 means malformed JQL, not
            // a RalphX-side validation failure, so it must surface through
            // the status-preserving Api mapping instead of a flat 400. No
            // silent raw→smart fallback.
            SearchMode::RawQuery => AtlassianMcpHttpError::Api(error),
            SearchMode::Smart => AtlassianMcpHttpError::InvalidRequest(error.message),
        })?;

    Ok(Json(JiraSearchResponse {
        issues: results
            .into_iter()
            .map(|summary| serde_json::to_value(summary).unwrap_or(serde_json::Value::Null))
            .collect(),
    }))
}

pub async fn jira_get_issue(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraIssueKeyRequest>,
) -> Result<Json<JiraIssueResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::Read).await?;
    let issue_key = required_field(&request.issue_key, "Jira issue key is required")?;

    let content = state
        .app_state
        .atlassian_integration_service
        .fetch_resource_content(&crate::domain::services::ComposerIntegrationReference {
            provider: "atlassian".to_string(),
            kind: "jira".to_string(),
            id: issue_key.clone(),
            key: Some(issue_key),
            title: None,
            url: None,
            summary_excerpt: None,
            include_transcript: None,
        })
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

    let mut issue = serde_json::to_value(content).unwrap_or(serde_json::Value::Null);
    redact_jira_attachment_download_urls(&mut issue);

    Ok(Json(JiraIssueResponse { issue }))
}

/// Strip `contentUrl`/`thumbnailUrl` from serialized Jira attachments before
/// they leave this MCP response boundary. `AtlassianResourceContent` keeps
/// those fields for the Tauri/UI path and `fetch_ticket_attachment`'s source
/// resolution, but any read-tier agent could otherwise read raw Jira
/// attachment download URLs straight out of `jira_get_issue` — the exact
/// URLs the dedicated ticket-attachment tools take care to strip.
fn redact_jira_attachment_download_urls(issue: &mut serde_json::Value) {
    let Some(attachments) = issue
        .get_mut("attachments")
        .and_then(|value| value.as_array_mut())
    else {
        return;
    };
    for attachment in attachments {
        if let Some(object) = attachment.as_object_mut() {
            object.remove("contentUrl");
            object.remove("thumbnailUrl");
        }
    }
}

pub async fn jira_list_projects(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraListProjectsRequest>,
) -> Result<Json<JiraProjectsResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::Read).await?;
    let limit = request.limit.unwrap_or(50).clamp(1, 200);

    let projects = state
        .app_state
        .atlassian_integration_service
        .list_jira_projects(limit)
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

    Ok(Json(JiraProjectsResponse {
        projects: projects
            .into_iter()
            .map(|project| serde_json::to_value(project).unwrap_or(serde_json::Value::Null))
            .collect(),
    }))
}

pub async fn jira_list_transitions(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraIssueKeyRequest>,
) -> Result<Json<JiraTransitionsResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::Read).await?;
    let issue_key = required_field(&request.issue_key, "Jira issue key is required")?;

    let transitions = state
        .app_state
        .atlassian_integration_service
        .list_jira_issue_transitions(&issue_key)
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

    Ok(Json(JiraTransitionsResponse {
        transitions: transitions
            .into_iter()
            .map(|transition| serde_json::to_value(transition).unwrap_or(serde_json::Value::Null))
            .collect(),
    }))
}

pub async fn jira_create_issue(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraCreateIssueRequest>,
) -> Result<Json<JiraCreateIssueResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::ReadWrite).await?;

    let issue = state
        .app_state
        .atlassian_integration_service
        .create_jira_issue(&JiraIssueCreateRequest {
            project_key: required_field(&request.project_key, "Jira project key is required")?,
            issue_type: required_field(&request.issue_type, "Jira issue type is required")?,
            summary: required_field(&request.summary, "Jira issue summary is required")?,
            description: request.description,
            labels: request.labels,
            priority: request.priority,
            parent_key: request.parent_key,
            assignee_account_id: request.assignee_account_id,
            components: request.components,
        })
        .await?;

    Ok(Json(JiraCreateIssueResponse { issue }))
}

pub async fn jira_update_issue(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraUpdateIssueRequest>,
) -> Result<Json<JiraAckResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::ReadWrite).await?;
    let issue_key = required_field(&request.issue_key, "Jira issue key is required")?;

    state
        .app_state
        .atlassian_integration_service
        .update_jira_issue(
            &issue_key,
            &JiraIssueUpdateRequest {
                summary: request.summary,
                description: request.description,
                labels: request.labels,
                priority: request.priority,
            },
        )
        .await?;

    Ok(Json(JiraAckResponse { ok: true }))
}

pub async fn jira_add_comment(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraAddCommentRequest>,
) -> Result<Json<JiraIssueResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::ReadWrite).await?;
    let issue_key = required_field(&request.issue_key, "Jira issue key is required")?;
    let body = required_field(&request.body, "Jira comment body is required")?;

    let comment = state
        .app_state
        .atlassian_integration_service
        .add_jira_comment(&issue_key, &body)
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

    Ok(Json(JiraIssueResponse {
        issue: serde_json::to_value(comment).unwrap_or(serde_json::Value::Null),
    }))
}

pub async fn jira_transition_issue(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraTransitionIssueRequest>,
) -> Result<Json<JiraAckResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::ReadWrite).await?;
    let issue_key = required_field(&request.issue_key, "Jira issue key is required")?;
    let transition_id = required_field(&request.transition_id, "Jira transition id is required")?;

    state
        .app_state
        .atlassian_integration_service
        .transition_jira_issue(&issue_key, &transition_id)
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

    Ok(Json(JiraAckResponse { ok: true }))
}

pub async fn jira_assign_issue(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraAssignIssueRequest>,
) -> Result<Json<JiraAckResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::ReadWrite).await?;
    let issue_key = required_field(&request.issue_key, "Jira issue key is required")?;

    let service = &state.app_state.atlassian_integration_service;
    // Precedence: explicit accountId > assignToMe > clear.
    let result = match request.account_id.as_deref() {
        Some(account_id) => {
            let account_id = required_field(account_id, "Jira accountId is required")?;
            service
                .assign_jira_issue_to_account(&issue_key, &account_id)
                .await
        }
        None if request.assign_to_me => {
            service.assign_jira_issue_to_current_user(&issue_key).await
        }
        None => service.clear_jira_issue_assignee(&issue_key).await,
    };
    result.map_err(AtlassianMcpHttpError::InvalidRequest)?;

    Ok(Json(JiraAckResponse { ok: true }))
}

pub async fn jira_list_comments(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraListCommentsRequest>,
) -> Result<Json<JiraListCommentsResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::Read).await?;
    let issue_key = required_field(&request.issue_key, "Jira issue key is required")?;
    let start_at = request.start_at.unwrap_or(0);
    let max_results = request
        .max_results
        .unwrap_or(20)
        .clamp(1, MAX_COMMENT_RESULTS);

    let page = state
        .app_state
        .atlassian_integration_service
        .list_jira_comments(&issue_key, start_at, max_results)
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

    Ok(Json(JiraListCommentsResponse {
        comments: page
            .comments
            .into_iter()
            .map(|comment| serde_json::to_value(comment).unwrap_or(serde_json::Value::Null))
            .collect(),
        total: page.total,
    }))
}

pub async fn jira_search_users(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraSearchUsersRequest>,
) -> Result<Json<JiraSearchUsersResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::Read).await?;
    let query = required_field(&request.query, "Jira user search query is required")?;
    let limit = request
        .max_results
        .unwrap_or(MAX_USER_SEARCH_RESULTS)
        .clamp(1, MAX_USER_SEARCH_RESULTS);

    let users = state
        .app_state
        .atlassian_integration_service
        .search_jira_users(&query, limit)
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

    Ok(Json(JiraSearchUsersResponse {
        users: users
            .into_iter()
            .map(|user| serde_json::to_value(user).unwrap_or(serde_json::Value::Null))
            .collect(),
    }))
}
