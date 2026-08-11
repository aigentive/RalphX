use super::*;
use crate::application::plan_verification_service::{
    get_plan_verification_status, request_plan_verification, PlanVerificationRequestSource,
    PlanVerificationStatus,
};

#[derive(Debug, Deserialize)]
pub struct TriggerVerificationRequest {
    pub session_id: String,
}

#[derive(Debug, Serialize)]
pub struct TriggerVerificationResponse {
    pub status: String,
    pub session_id: String,
}

/// POST /api/external/trigger_verification
pub async fn trigger_verification_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<TriggerVerificationRequest>,
) -> Result<Json<TriggerVerificationResponse>, StatusCode> {
    let session_id = IdeationSessionId::from_string(req.session_id);
    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|error| {
            error!(%error, session_id = %session_id, "Failed to load verification session");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    session
        .assert_project_scope(&scope)
        .map_err(|error| error.status)?;

    let chat_service = state
        .app_state
        .build_chat_service_with_execution_state(state.execution_state.clone());
    let outcome = request_plan_verification(
        &state.app_state,
        &chat_service,
        &session_id,
        PlanVerificationRequestSource::External,
    )
    .await
    .map_err(|error| {
        error!(%error, session_id = %session_id, "Failed to request plan verification");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(TriggerVerificationResponse {
        status: outcome.as_str().to_string(),
        session_id: session_id.as_str().to_string(),
    }))
}

/// GET /api/external/plan_verification/:session_id
pub async fn get_plan_verification_external_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(session_id): Path<String>,
) -> Result<Json<PlanVerificationStatus>, StatusCode> {
    let session_id = IdeationSessionId::from_string(session_id);
    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|error| {
            error!(%error, session_id = %session_id, "Failed to load verification session");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    session
        .assert_project_scope(&scope)
        .map_err(|error| error.status)?;

    get_plan_verification_status(&state.app_state, &session_id)
        .await
        .map(Json)
        .map_err(|error| {
            error!(%error, session_id = %session_id, "Failed to load plan verification status");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}
