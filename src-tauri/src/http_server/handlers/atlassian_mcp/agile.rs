//! Jira Software (Agile) endpoints for the Atlassian MCP tool surface: boards,
//! sprints, and sprint issues.
//!
//! Schemas never accept run, conversation, or orchestration ids: caller
//! identity is transport-owned and read from headers.

use axum::{extract::State, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};

use crate::domain::agents::AtlassianMcpAccess;
use crate::http_server::HttpServerState;

use super::{authorize, required_field, AtlassianMcpHttpError};

/// Upper bound on sprint issues returned by `jira_get_sprint_issues`, matching
/// the tool schema.
const MAX_SPRINT_ISSUES: usize = 50;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraListBoardsRequest {
    /// Optional project key filter. Omitted or blank lists every board.
    #[serde(default)]
    pub project_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JiraBoardsResponse {
    pub boards: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraListSprintsRequest {
    pub board_id: String,
    /// Only `"active"` (the default) is currently supported; any other value
    /// is rejected rather than silently ignored.
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JiraSprintsResponse {
    pub sprints: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraGetSprintIssuesRequest {
    pub sprint_id: String,
}

#[derive(Debug, Serialize)]
pub struct JiraSprintIssuesResponse {
    pub issues: Vec<serde_json::Value>,
}

pub async fn jira_list_boards(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraListBoardsRequest>,
) -> Result<Json<JiraBoardsResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::Read).await?;
    let project_key = request.project_key.unwrap_or_default();

    let boards = state
        .app_state
        .atlassian_integration_service
        .list_jira_boards(&project_key)
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

    Ok(Json(JiraBoardsResponse {
        boards: boards
            .into_iter()
            .map(|board| serde_json::to_value(board).unwrap_or(serde_json::Value::Null))
            .collect(),
    }))
}

pub async fn jira_list_sprints(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraListSprintsRequest>,
) -> Result<Json<JiraSprintsResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::Read).await?;
    let board_id = required_field(&request.board_id, "Jira board id is required")?;
    if let Some(state_filter) = request.state.as_deref() {
        if state_filter.trim() != "active" {
            return Err(AtlassianMcpHttpError::InvalidRequest(format!(
                "Unsupported Jira sprint state '{state_filter}': only 'active' is available"
            )));
        }
    }

    let sprints = state
        .app_state
        .atlassian_integration_service
        .list_jira_active_sprints(&board_id)
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

    Ok(Json(JiraSprintsResponse {
        sprints: sprints
            .into_iter()
            .map(|sprint| serde_json::to_value(sprint).unwrap_or(serde_json::Value::Null))
            .collect(),
    }))
}

pub async fn jira_get_sprint_issues(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<JiraGetSprintIssuesRequest>,
) -> Result<Json<JiraSprintIssuesResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::Read).await?;
    let sprint_id = required_field(&request.sprint_id, "Jira sprint id is required")?;

    let issues = state
        .app_state
        .atlassian_integration_service
        .list_jira_sprint_issues(&sprint_id, MAX_SPRINT_ISSUES)
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

    Ok(Json(JiraSprintIssuesResponse {
        issues: issues
            .into_iter()
            .map(|issue| serde_json::to_value(issue).unwrap_or(serde_json::Value::Null))
            .collect(),
    }))
}
