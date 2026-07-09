use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use tracing::error;

use crate::domain::entities::{
    IdeationSessionId, ProjectId, VerificationConfirmationStatus,
};
use crate::error::AppError;

use super::super::types::{
    AutoAcceptVerificationRequest, ConfirmVerificationRequest, ConfirmationStatusResponse,
    DismissVerificationRequest, HttpError, HttpServerState, PendingVerificationConfirmationItem,
    PendingVerificationConfirmationsResponse, SpecialistEntryResponse, SpecialistsResponse,
    VerificationActionResponse,
};

mod auto_accept;
mod confirm;
mod confirmation_status;
mod dismiss;
mod helpers;
mod pending_confirmations;
mod specialist_registry;

pub use auto_accept::set_auto_accept_verification;
pub use confirm::confirm_verification;
pub use confirmation_status::get_confirmation_status;
pub use dismiss::dismiss_verification;
pub use helpers::handle_verification_spawn_failure as handle_spawn_failure;
pub use helpers::spawn_verification_agent;
pub use pending_confirmations::get_pending_verification_confirmations;
pub use specialist_registry::get_verification_specialists;

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
