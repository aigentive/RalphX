//! GET /api/managed_team/members/status — coordinator-only liveness projection.
//!
//! Joins the durable Team roster (`ManagedTeamService::roster`) to delegated-session
//! liveness (`build_delegated_session_status_response`). Never reimplements either;
//! a member whose delegated session was already cleared, or whose status lookup
//! 404s, degrades to an entry with no liveness fields rather than dropping the
//! member or failing the whole request. Repository errors still propagate as 500s.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::http_server::handlers::coordination::build_delegated_session_status_response;
use crate::http_server::handlers::managed_team::members::resolve_coordinator_authority;
use crate::http_server::types::{HttpServerState, ManagedTeamMemberStatusEntry};

type JsonError = (StatusCode, Json<serde_json::Value>);

fn json_error(status: StatusCode, error: impl Into<String>) -> JsonError {
    (
        status,
        Json(serde_json::json!({
            "status": status.as_u16(),
            "error": error.into(),
        })),
    )
}

fn enum_wire_value<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

pub async fn get_managed_team_member_status(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ManagedTeamMemberStatusEntry>>, JsonError> {
    let authority = resolve_coordinator_authority(&state, &headers).await?;
    let members = state
        .app_state
        .managed_team
        .roster(&authority.team_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    let mut entries = Vec::with_capacity(members.len().min(32));
    for member in members.into_iter().take(32) {
        let mut entry = ManagedTeamMemberStatusEntry {
            name: member.name,
            normalized_name: member.normalized_name,
            role_summary: member.role_summary,
            status: enum_wire_value(&member.status),
            canonical_agent_name: member.canonical_agent_name,
            generation: member.generation,
            harness: member.harness.map(|harness| harness.to_string()),
            current_assignment_id: member
                .current_assignment_id
                .as_ref()
                .map(|id| id.as_str().to_string()),
            last_activity_at: member.last_activity_at.map(|at| at.to_rfc3339()),
            is_running: None,
            last_active_at: None,
            estimated_status: None,
            latest_run: None,
        };

        if let Some(delegated_session_id) = member.delegated_session_id.as_ref() {
            let current_run_id = member.current_run_id.as_ref().map(|id| id.as_str());
            match build_delegated_session_status_response(
                &state,
                delegated_session_id.as_str(),
                false,
                None,
                current_run_id.as_deref(),
            )
            .await
            {
                Ok(status) => {
                    entry.is_running = Some(status.agent_state.is_running);
                    entry.last_active_at = status.agent_state.last_active_at;
                    entry.estimated_status = Some(status.agent_state.estimated_status);
                    entry.latest_run = status.latest_run;
                }
                Err((StatusCode::NOT_FOUND, _)) => {
                    // Delegated session was already cleared: degrade, never fail.
                }
                Err(error) => return Err(error),
            }
        }

        entries.push(entry);
    }

    Ok(Json(entries))
}
