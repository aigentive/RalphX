use super::*;
use crate::application::plan_verification_service::complete_plan_verification;
use crate::domain::entities::{AgentRunActionKind, AgentRunId};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CompletePlanVerificationResponse {
    pub status: &'static str,
    pub plan_artifact_id: String,
}

pub async fn complete_plan_verification_http(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<CompletePlanVerificationResponse>, HttpError> {
    let run_id = headers
        .get("x-ralphx-agent-run-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| HttpError::validation("Missing verification run authority".to_string()))?;
    let conversation_id = headers
        .get("x-ralphx-conversation-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            HttpError::validation("Missing verification conversation authority".to_string())
        })?;

    let run = state
        .app_state
        .agent_run_repo
        .get_by_id(&AgentRunId::from_string(run_id))
        .await
        .map_err(map_app_err_local)?
        .ok_or_else(|| HttpError::from(StatusCode::NOT_FOUND))?;
    if run.conversation_id.as_str() != conversation_id
        || run.action_kind != Some(AgentRunActionKind::VerifyPlan)
    {
        return Err(HttpError {
            status: StatusCode::CONFLICT,
            message: Some("Stale or ordinary run cannot complete plan verification".to_string()),
        });
    }
    let session_id = run
        .action_context_id
        .as_deref()
        .map(|value| IdeationSessionId::from_string(value.to_string()))
        .ok_or_else(|| {
            HttpError::validation("Verification run is missing its session binding".to_string())
        })?;
    let artifact_id = complete_plan_verification(&state.app_state, &session_id, run_id)
        .await
        .map_err(map_app_err_local)?;

    Ok(Json(CompletePlanVerificationResponse {
        status: "verified",
        plan_artifact_id: artifact_id,
    }))
}
