//! Jira endpoints for the Atlassian MCP tool surface.
//!
//! Schemas never accept run, conversation, or orchestration ids: caller
//! identity is transport-owned and read from headers.

use axum::{extract::State, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};

use crate::application::{
    AtlassianResourceKind, JiraIssueCreateRequest, JiraIssueCreated, JiraIssueUpdateRequest,
};
use crate::domain::agents::AtlassianMcpAccess;
use crate::http_server::HttpServerState;

use super::{authorize, required_field, AtlassianMcpHttpError};

/// Upper bound on Jira search results, mirroring the MCP tool schema.
const MAX_SEARCH_RESULTS: usize = 50;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraSearchRequest {
    pub query: String,
    #[serde(default)]
    pub max_results: Option<usize>,
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
    /// When false, the current assignee is cleared instead of set.
    #[serde(default)]
    pub assign_to_me: bool,
}

#[derive(Debug, Serialize)]
pub struct JiraAckResponse {
    pub ok: bool,
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

    let results = state
        .app_state
        .atlassian_integration_service
        .search_resources(AtlassianResourceKind::Jira, &query, limit)
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

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

    Ok(Json(JiraIssueResponse {
        issue: serde_json::to_value(content).unwrap_or(serde_json::Value::Null),
    }))
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
    let result = if request.assign_to_me {
        service.assign_jira_issue_to_current_user(&issue_key).await
    } else {
        service.clear_jira_issue_assignee(&issue_key).await
    };
    result.map_err(AtlassianMcpHttpError::InvalidRequest)?;

    Ok(Json(JiraAckResponse { ok: true }))
}
