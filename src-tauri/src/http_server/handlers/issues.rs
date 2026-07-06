use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use tracing::error;

use super::*;
use crate::commands::review_commands_types::{IssueProgressResponse, ReviewIssueResponse};
use crate::domain::entities::{ReviewIssueId, TaskId};

/// Optional query params for get_task_issues
#[derive(Debug, Deserialize)]
pub struct TaskIssuesQuery {
    pub status: Option<String>,
}

pub async fn get_task_issues_http(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
    Query(query): Query<TaskIssuesQuery>,
) -> Result<Json<Vec<ReviewIssueResponse>>, StatusCode> {
    let task_id = TaskId::from_string(task_id);

    let issues = match query.status.as_deref() {
        Some("open") => state
            .app_state
            .review_issue_repo
            .get_open_by_task_id(&task_id)
            .await
            .map_err(|e| {
                error!(
                    "Failed to get open issues for task {}: {}",
                    task_id.as_str(),
                    e
                );
                StatusCode::INTERNAL_SERVER_ERROR
            })?,
        _ => state
            .app_state
            .review_issue_repo
            .get_by_task_id(&task_id)
            .await
            .map_err(|e| {
                error!("Failed to get issues for task {}: {}", task_id.as_str(), e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?,
    };

    Ok(Json(
        issues.into_iter().map(ReviewIssueResponse::from).collect(),
    ))
}

pub async fn get_issue_progress_http(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
) -> Result<Json<IssueProgressResponse>, StatusCode> {
    let task_id = TaskId::from_string(task_id);

    let summary = state
        .app_state
        .review_issue_repo
        .get_summary(&task_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get issue progress for task {}: {}",
                task_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(IssueProgressResponse::from(summary)))
}

pub async fn mark_issue_in_progress_http(
    State(state): State<HttpServerState>,
    Json(req): Json<MarkIssueInProgressRequest>,
) -> Result<Json<ReviewIssueResponse>, StatusCode> {
    let issue_id = ReviewIssueId::from_string(req.issue_id);

    let mut issue = state
        .app_state
        .review_issue_repo
        .get_by_id(&issue_id)
        .await
        .map_err(|e| {
            error!("Failed to get issue {}: {}", issue_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if issue.status != crate::domain::entities::IssueStatus::Open {
        return Err(StatusCode::BAD_REQUEST);
    }

    issue.start_work();
    state
        .app_state
        .review_issue_repo
        .update(&issue)
        .await
        .map_err(|e| {
            error!("Failed to update issue {}: {}", issue_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let response = ReviewIssueResponse::from(issue);

    crate::http_server::emit_http_event(
        &state,
        "issue:updated",
        serde_json::json!({
            "issue": &response,
            "task_id": &response.task_id
        }),
    );

    Ok(Json(response))
}

pub async fn mark_issue_addressed_http(
    State(state): State<HttpServerState>,
    Json(req): Json<MarkIssueAddressedRequest>,
) -> Result<Json<ReviewIssueResponse>, StatusCode> {
    let issue_id = ReviewIssueId::from_string(req.issue_id);

    let mut issue = state
        .app_state
        .review_issue_repo
        .get_by_id(&issue_id)
        .await
        .map_err(|e| {
            error!("Failed to get issue {}: {}", issue_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if !issue.needs_work() {
        return Err(StatusCode::BAD_REQUEST);
    }

    issue.mark_addressed(Some(req.resolution_notes), req.attempt_number);
    state
        .app_state
        .review_issue_repo
        .update(&issue)
        .await
        .map_err(|e| {
            error!("Failed to update issue {}: {}", issue_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let response = ReviewIssueResponse::from(issue);

    crate::http_server::emit_http_event(
        &state,
        "issue:updated",
        serde_json::json!({
            "issue": &response,
            "task_id": &response.task_id
        }),
    );

    Ok(Json(response))
}
