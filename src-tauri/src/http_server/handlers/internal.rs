// Internal HTTP handlers — no CORS, localhost-only access
//
// These endpoints are called by the internal MCP server (ralphx-mcp-server)
// running as a child process on the same machine. Since these are not browser
// clients, CORS is not required or applied.
//
// Security posture: same as all other /api/* routes — server binds to
// 127.0.0.1:3847 (localhost-only). No auth token required; access is
// restricted by network topology (only processes on the same machine can
// reach this server).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;

use super::*;
use crate::commands::ideation_commands::{
    create_cross_project_session_impl, migrate_proposals_impl, CreateCrossProjectSessionInput,
    IdeationSessionResponse, MigrateProposalsInput, MigrateProposalsResult,
};

// ============================================================================
// GET /api/internal/projects — response types
// ============================================================================

/// Internal-only project summary that includes the filesystem path.
///
/// Intentionally separate from `ProjectSummary` (external.rs) which must NOT
/// expose filesystem paths to external API key holders.
#[derive(Debug, Serialize)]
pub struct InternalProjectSummary {
    pub id: String,
    pub name: String,
    pub working_directory: String,
    pub task_count: u32,
}

// ============================================================================
// GET /api/internal/projects
// ============================================================================

/// Returns all projects without scope filtering, including filesystem paths.
///
/// Unlike `GET /api/external/projects` (which requires a `ProjectScope` header
/// and filters by it), this endpoint returns every project in the database.
/// It is used by the internal MCP server to let ideation agents discover
/// available projects when orchestrating cross-project session creation.
///
/// The response includes `working_directory` (filesystem path) which is
/// intentionally excluded from the external endpoint for security reasons.
pub async fn list_projects_internal(
    State(state): State<HttpServerState>,
) -> Result<Json<Vec<InternalProjectSummary>>, StatusCode> {
    let projects = state.app_state.project_repo.get_all().await.map_err(|e| {
        tracing::error!("list_projects_internal: failed to query projects: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut summaries = Vec::new();
    for project in &projects {
        let task_count = state
            .app_state
            .task_repo
            .count_tasks(&project.id, false, None, None)
            .await
            .unwrap_or(0);

        summaries.push(InternalProjectSummary {
            id: project.id.to_string(),
            name: project.name.clone(),
            working_directory: project.working_directory.clone(),
            task_count,
        });
    }

    Ok(Json(summaries))
}

// ============================================================================
// POST /api/internal/cross_project/create_session
// ============================================================================

/// Creates a new ideation session in a target project by inheriting a verified
/// plan from a source session. Delegates to the shared
/// `create_cross_project_session_impl` function (same logic used by the Tauri
/// IPC command) to avoid code duplication.
///
/// Used by the internal MCP server's `create_cross_project_session` tool.
///
/// # Errors
///
/// - `400 Bad Request` — source plan is not verified, or no plan artifact.
/// - `422 Unprocessable Entity` — circular import detected, chain too deep,
///   or target path does not exist on disk.
/// - `500 Internal Server Error` — unexpected database or application error.
pub async fn create_cross_project_session_http(
    State(state): State<HttpServerState>,
    Json(input): Json<CreateCrossProjectSessionInput>,
) -> Result<Json<IdeationSessionResponse>, HttpError> {
    create_cross_project_session_impl(&state.app_state, state.app_state.events.as_ref(), input)
        .await
        .map(Json)
        .map_err(|e| {
            // Map well-known error prefixes to appropriate HTTP status codes.
            // `create_cross_project_session_impl` returns plain String errors
            // (matching the Tauri command contract).
            if e.contains("CIRCULAR_IMPORT")
                || e.contains("SELF_REFERENCE")
                || e.contains("CHAIN_TOO_DEEP")
            {
                tracing::warn!(
                    "create_cross_project_session_http: circular import rejected: {}",
                    e
                );
                HttpError::validation(e)
            } else if e.contains("not verified") || e.contains("no plan artifact") {
                HttpError {
                    status: StatusCode::BAD_REQUEST,
                    message: Some(e),
                }
            } else if e.contains("does not exist on disk") {
                HttpError::validation(e)
            } else {
                tracing::error!("create_cross_project_session_http: unexpected error: {}", e);
                HttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: Some(e),
                }
            }
        })
}

// ============================================================================
// POST /api/internal/cross_project/migrate_proposals
// ============================================================================

/// Migrates proposals from a source session to a target session.
/// Delegates to the shared `migrate_proposals_impl` function (same logic used by the Tauri
/// IPC command) to avoid code duplication.
///
/// Used by the internal MCP server's `migrate_proposals` tool.
///
/// # Errors
///
/// - `404 Not Found` — source or target session not found.
/// - `500 Internal Server Error` — unexpected database or application error.
pub async fn migrate_proposals_http(
    State(state): State<HttpServerState>,
    Json(input): Json<MigrateProposalsInput>,
) -> Result<Json<MigrateProposalsResult>, HttpError> {
    migrate_proposals_impl(&state.app_state, input)
        .await
        .map(Json)
        .map_err(|e| {
            if e.contains("not found") {
                HttpError {
                    status: StatusCode::NOT_FOUND,
                    message: Some(e),
                }
            } else {
                tracing::error!("migrate_proposals_http: unexpected error: {}", e);
                HttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: Some(e),
                }
            }
        })
}

// ============================================================================
// POST /api/internal/sessions/:id/cross_project_check
// ============================================================================

/// Sets cross_project_checked = true on the given ideation session.
/// Called by the cross_project_guide MCP tool after analysis is complete.
///
/// # Errors
///
/// - `404 Not Found` — session ID does not exist.
/// - `500 Internal Server Error` — database error.
pub async fn set_cross_project_checked(
    State(state): State<HttpServerState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let rows_affected = state
        .app_state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions SET cross_project_checked = 1, updated_at = datetime('now') WHERE id = ?1",
                [&id],
            )
            .map_err(|e| crate::error::AppError::Database(e.to_string()))
        })
        .await
        .map_err(|e| {
            tracing::error!("set_cross_project_checked: DB error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?;

    if rows_affected == 0 {
        Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Session not found" })),
        ))
    } else {
        Ok(Json(serde_json::json!({ "status": "ok" })))
    }
}
