use axum::{
    extract::{Path, State},
    Json,
};
use tracing::error;

use crate::application::plan_complexity_assessment::{
    emit_plan_complexity_assessed, get_current_plan_complexity_assessment_sync,
    upsert_plan_complexity_assessment_sync,
};
use crate::error::AppError;
use crate::http_server::types::{
    HttpError, HttpServerState, PlanComplexityAssessmentResponse,
    SubmitPlanComplexityAssessmentRequest, SubmitPlanComplexityAssessmentResponse,
};

const PLAN_COMPLEXITY_ASSESSOR: &str = "ralphx-utility-plan-complexity";

pub async fn get_plan_complexity_assessment(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
) -> Result<Json<Option<PlanComplexityAssessmentResponse>>, HttpError> {
    let assessment = state
        .app_state
        .db
        .run(move |conn| get_current_plan_complexity_assessment_sync(conn, &session_id))
        .await
        .map_err(|error| {
            error!("get_plan_complexity_assessment failed: {}", error);
            map_plan_complexity_error(error)
        })?;

    Ok(Json(assessment))
}

pub async fn submit_plan_complexity_assessment(
    State(state): State<HttpServerState>,
    Json(req): Json<SubmitPlanComplexityAssessmentRequest>,
) -> Result<Json<SubmitPlanComplexityAssessmentResponse>, HttpError> {
    let assessment = state
        .app_state
        .db
        .run(move |conn| {
            upsert_plan_complexity_assessment_sync(conn, req, PLAN_COMPLEXITY_ASSESSOR)
        })
        .await
        .map_err(|error| {
            error!("submit_plan_complexity_assessment failed: {}", error);
            map_plan_complexity_error(error)
        })?;

    emit_plan_complexity_assessed(&state.app_state, &assessment);

    Ok(Json(SubmitPlanComplexityAssessmentResponse {
        success: true,
        assessment,
    }))
}

fn map_plan_complexity_error(error: AppError) -> HttpError {
    match error {
        AppError::Validation(message) => HttpError::validation(message),
        AppError::Conflict(message) => HttpError {
            status: axum::http::StatusCode::CONFLICT,
            message: Some(message),
        },
        AppError::FeatureDisabled(message) => HttpError {
            status: axum::http::StatusCode::CONFLICT,
            message: Some(message),
        },
        AppError::NotFound(_) => axum::http::StatusCode::NOT_FOUND.into(),
        _ => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into(),
    }
}
