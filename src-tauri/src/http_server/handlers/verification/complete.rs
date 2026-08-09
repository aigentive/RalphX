use super::*;
use crate::application::plan_verification_service::{
    complete_plan_verification_with_deps, PlanVerificationServiceDeps,
};
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
    let verification_deps = PlanVerificationServiceDeps::from_app_state(&state.app_state);
    let completion = complete_plan_verification_with_deps(&verification_deps, &session_id, run_id)
        .await
        .map_err(map_app_err_local)?;

    if completion.newly_recorded {
        if let Some(session) = state
            .app_state
            .ideation_session_repo
            .get_by_id(&session_id)
            .await
            .map_err(map_app_err_local)?
        {
            let project_id = session.project_id.as_str().to_string();
            let payload = serde_json::json!({
                "session_id": session_id.as_str(),
                "project_id": project_id,
                "plan_artifact_id": completion.artifact_id.clone(),
                "status": "verified",
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            if let Some(publisher) = state.app_state.webhook_publisher.as_ref() {
                if let Err(error) = crate::domain::services::emit_external_webhook_event(
                    "ideation:verified",
                    session.project_id.as_str(),
                    payload,
                    &state.app_state.external_events_repo,
                    publisher,
                )
                .await
                {
                    tracing::warn!(%error, "Failed to emit ideation:verified webhook event");
                }
            } else if let Err(error) = state
                .app_state
                .external_events_repo
                .insert_event(
                    "ideation:verified",
                    session.project_id.as_str(),
                    &payload.to_string(),
                )
                .await
            {
                tracing::warn!(%error, "Failed to persist ideation:verified event");
            }
        }
    }

    Ok(Json(CompletePlanVerificationResponse {
        status: "verified",
        plan_artifact_id: completion.artifact_id,
    }))
}
