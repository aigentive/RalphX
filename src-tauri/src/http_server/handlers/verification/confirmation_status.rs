use super::*;
use crate::application::harness_runtime_registry::default_verification_specialists;

pub async fn get_confirmation_status(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
) -> Result<Json<ConfirmationStatusResponse>, HttpError> {
    let sid = IdeationSessionId::from_string(session_id.clone());
    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&sid)
        .await
        .map_err(map_app_err_local)?
        .ok_or_else(|| HttpError::from(StatusCode::NOT_FOUND))?;

    match session.verification_confirmation_status {
        Some(VerificationConfirmationStatus::Pending) => {
            let available_specialists = default_verification_specialists()
                .iter()
                .map(|s| SpecialistEntryResponse {
                    name: s.name.clone(),
                    display_name: s.display_name.clone(),
                    description: s.description.clone(),
                    dispatch_mode: s.dispatch_mode.clone(),
                    enabled_by_default: s.enabled_by_default,
                })
                .collect();
            Ok(Json(ConfirmationStatusResponse {
                session_id,
                status: "pending".to_string(),
                plan_artifact_id: session.plan_artifact_id.map(|id| id.as_str().to_string()),
                available_specialists: Some(available_specialists),
            }))
        }
        Some(VerificationConfirmationStatus::Accepted) => Ok(Json(ConfirmationStatusResponse {
            session_id,
            status: "accepted".to_string(),
            plan_artifact_id: None,
            available_specialists: None,
        })),
        Some(VerificationConfirmationStatus::Rejected) => Ok(Json(ConfirmationStatusResponse {
            session_id,
            status: "rejected".to_string(),
            plan_artifact_id: None,
            available_specialists: None,
        })),
        None => Ok(Json(ConfirmationStatusResponse {
            session_id,
            status: "not_applicable".to_string(),
            plan_artifact_id: None,
            available_specialists: None,
        })),
    }
}
