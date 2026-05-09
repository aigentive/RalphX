use std::sync::Arc;

use super::*;
use crate::application::spawn_ready_task_scheduler_if_needed;
use crate::commands::ideation_commands::{
    append_ideation_plan_task_core, AppendIdeationPlanTaskInput, AppendIdeationPlanTaskResult,
};
use crate::http_server::handlers::ideation::append_error_to_http;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAppendSessionTaskRequest {
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub depends_on_task_ids: Vec<String>,
    pub priority: Option<i32>,
    pub source_conversation_id: Option<String>,
    pub source_message_id: Option<String>,
}

/// POST /api/external/sessions/:session_id/tasks
///
/// Append a one-off execution task to an already accepted ideation plan.
pub async fn append_session_task_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(session_id): Path<String>,
    Json(req): Json<ExternalAppendSessionTaskRequest>,
) -> Result<Json<AppendIdeationPlanTaskResult>, HttpError> {
    let session_id_obj = IdeationSessionId::from_string(session_id.clone());
    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_obj)
        .await
        .map_err(|e| {
            error!("Failed to get ideation session {}: {}", session_id, e);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to get ideation session".to_string()),
            }
        })?
        .ok_or(HttpError {
            status: StatusCode::NOT_FOUND,
            message: Some("Session not found".to_string()),
        })?;

    session.assert_project_scope(&scope)?;

    let result = append_ideation_plan_task_core(
        &state.app_state,
        AppendIdeationPlanTaskInput {
            project_id: Some(session.project_id.as_str().to_string()),
            session_id,
            title: req.title,
            description: req.description,
            steps: req.steps,
            acceptance_criteria: req.acceptance_criteria,
            depends_on_task_ids: req.depends_on_task_ids,
            priority: req.priority,
            source_conversation_id: req.source_conversation_id,
            source_message_id: req.source_message_id,
        },
    )
    .await
    .map_err(append_error_to_http)?;

    spawn_ready_task_scheduler_if_needed(
        &state.app_state,
        Arc::clone(&state.execution_state),
        state.app_state.app_handle.as_ref().cloned(),
        result.any_ready_tasks,
    );

    Ok(Json(result))
}
