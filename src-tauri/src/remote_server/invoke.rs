//! Bearer-authenticated command invocation over the remote facade.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use ralphx_remote_protocol::ErrorCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::remote_server::auth::RemoteIdentity;
use crate::remote_server::endpoints::RemoteRouterState;
use crate::remote_server::registry::{self, DispatchOutcome, RemoteInvokeError};
use crate::remote_server::remote_error_response;

/// Host-side mirror of the client wire type (C-11).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InvokeWireRequest {
    pub request_id: String,
    pub cmd: String,
    pub args: Value,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum InvokeWireResponse {
    Ok { ok: bool, result: Value },
    Err { ok: bool, error: Value },
}

pub(crate) async fn invoke_handler(
    State(state): State<RemoteRouterState>,
    Extension(identity): Extension<RemoteIdentity>,
    Json(request): Json<InvokeWireRequest>,
) -> axum::response::Response {
    // Reserved for PR 1.5 deduplication. Deserializing it here keeps the v1 wire contract exact.
    let _request_id = request.request_id;
    match registry::dispatch(
        state.app_handle(),
        identity.scopes.as_slice(),
        &request.cmd,
        &request.args,
    )
    .await
    {
        Ok(outcome) => dispatch_outcome_response(outcome),
        Err(error) => invoke_error_response(error),
    }
}

pub(crate) fn dispatch_outcome_response(outcome: DispatchOutcome) -> axum::response::Response {
    match outcome {
        DispatchOutcome::Ok(result) => (
            StatusCode::OK,
            Json(InvokeWireResponse::Ok { ok: true, result }),
        )
            .into_response(),
        DispatchOutcome::Err(error) => (
            StatusCode::OK,
            Json(InvokeWireResponse::Err { ok: false, error }),
        )
            .into_response(),
    }
}

pub(crate) fn invoke_error_response(error: RemoteInvokeError) -> axum::response::Response {
    remote_error_response(status_for_error_code(error.code), error.code, error.message)
}

/// Total host mapping, mirrored by the client's `status_error_code` reverse mapping.
pub(crate) const fn status_for_error_code(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::RemoteUnauthorized => StatusCode::UNAUTHORIZED,
        ErrorCode::RemoteForbidden => StatusCode::FORBIDDEN,
        ErrorCode::RemoteCommandUnavailable => StatusCode::NOT_FOUND,
        ErrorCode::RemoteTimeoutUnknown => StatusCode::REQUEST_TIMEOUT,
        ErrorCode::RemoteRequestInProgress => StatusCode::CONFLICT,
        ErrorCode::RemoteRequestIdReused => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::RemoteVersionMismatch => StatusCode::UPGRADE_REQUIRED,
        ErrorCode::RemoteUnreachable => StatusCode::SERVICE_UNAVAILABLE,
    }
}
