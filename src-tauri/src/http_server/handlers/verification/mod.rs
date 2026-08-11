use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::domain::entities::IdeationSessionId;
use crate::error::AppError;

use super::super::types::{
    ConfirmVerificationRequest, HttpError, HttpServerState, VerificationActionResponse,
};

mod complete;
mod confirm;

pub use complete::complete_plan_verification_http;
pub use confirm::confirm_verification;

/// Map an AppError to an HttpError for verification handler responses.
fn map_app_err_local(e: AppError) -> HttpError {
    match e {
        AppError::Validation(msg) => HttpError::validation(msg),
        AppError::NotFound(_) => StatusCode::NOT_FOUND.into(),
        AppError::Conflict(msg) => HttpError {
            status: StatusCode::CONFLICT,
            message: Some(msg),
        },
        _ => StatusCode::INTERNAL_SERVER_ERROR.into(),
    }
}
