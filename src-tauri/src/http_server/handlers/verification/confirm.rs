use super::*;
use crate::application::plan_verification_service::{
    request_plan_verification, PlanVerificationRequestSource,
};

pub async fn confirm_verification(
    State(state): State<HttpServerState>,
    Json(req): Json<ConfirmVerificationRequest>,
) -> Result<Json<VerificationActionResponse>, HttpError> {
    let session_id = IdeationSessionId::from_string(req.session_id);
    let chat_service = state
        .app_state
        .build_chat_service_with_execution_state(state.execution_state.clone());
    let outcome = request_plan_verification(
        &state.app_state,
        &chat_service,
        &session_id,
        PlanVerificationRequestSource::Manual,
    )
    .await
    .map_err(map_app_err_local)?;

    Ok(Json(VerificationActionResponse {
        status: outcome.as_str().to_string(),
    }))
}
