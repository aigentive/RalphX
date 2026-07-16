use super::*;
use crate::application::plan_verification_service::{
    get_plan_verification_status, PlanVerificationStatus,
};

/// GET /api/ideation/sessions/:id/verification
pub async fn get_plan_verification(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(session_id): Path<String>,
) -> Result<Json<PlanVerificationStatus>, JsonError> {
    let session_id = IdeationSessionId::from_string(session_id);
    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Session not found"))?;
    if !scope.is_unrestricted() {
        session
            .assert_project_scope(&scope)
            .map_err(|_| json_error(StatusCode::FORBIDDEN, "Forbidden"))?;
    }

    get_plan_verification_status(&state.app_state, &session_id)
        .await
        .map(Json)
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}
