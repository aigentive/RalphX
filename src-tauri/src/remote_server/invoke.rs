//! Bearer-authenticated command invocation over the remote facade.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use ralphx_remote_protocol::{ErrorCode, Scope};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::entities::{RemoteDedupOutcomeKind, RemoteRequestDedupRecord};
use crate::remote_server::auth::RemoteIdentity;
use crate::remote_server::dedup::{class_requires_dedup, DedupDecision, RecordedOutcome};
use crate::remote_server::endpoints::RemoteRouterState;
use crate::remote_server::registry::{self, DispatchOutcome, RemoteInvokeError};
use crate::remote_server::remote_error_response;

#[async_trait]
pub(crate) trait RemoteInvokeDispatcher: Send + Sync {
    async fn dispatch(
        &self,
        scopes: &[Scope],
        command: &str,
        args: &Value,
    ) -> Result<DispatchOutcome, RemoteInvokeError>;
}

pub(crate) struct TauriRemoteInvokeDispatcher {
    app_handle: tauri::AppHandle,
}

impl TauriRemoteInvokeDispatcher {
    pub(crate) fn shared(app_handle: tauri::AppHandle) -> Arc<dyn RemoteInvokeDispatcher> {
        Arc::new(Self { app_handle })
    }
}

#[async_trait]
impl RemoteInvokeDispatcher for TauriRemoteInvokeDispatcher {
    async fn dispatch(
        &self,
        scopes: &[Scope],
        command: &str,
        args: &Value,
    ) -> Result<DispatchOutcome, RemoteInvokeError> {
        registry::dispatch(&self.app_handle, scopes, command, args).await
    }
}

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

/// Bearer-authenticated command invocation, deduplicated per `(device, requestId)` (§4.3).
///
/// Order of operations — every step before any dispatch:
///
/// 1. `find_spec`: an unknown command bypasses dedup entirely, so a typo never burns a request
///    id that the client will legitimately reuse once corrected.
/// 2. `Read` class dispatches without dedup (exempt by spec — no side effect to make
///    at-most-once, and caching would serve stale reads).
/// 3. Mutating: consult the DURABLE store, then claim the in-flight slot ([`RemoteDedupState::begin`]),
///    which hands back an owned RAII reservation.
/// 4. Dispatch runs in a SPAWNED continuation that owns the reservation, so a dropped HTTP
///    connection (Axum cancels the handler future) neither aborts the command mid-effect nor
///    leaks the slot: the continuation finishes, and the reservation is released by `Drop`.
/// 5. Completed outcome (`Ok` **or** `Err`) → durable record, then release. Facade error →
///    release WITHOUT a record, because nothing executed — a corrected retry may reuse the id.
pub(crate) async fn invoke_handler(
    State(state): State<RemoteRouterState>,
    Extension(identity): Extension<RemoteIdentity>,
    Json(request): Json<InvokeWireRequest>,
) -> axum::response::Response {
    let InvokeWireRequest {
        request_id,
        cmd,
        args,
    } = request;

    // An unknown command is resolved before any reservation: dispatch will 404 on its own, and
    // burning the id here would make the client's corrected retry fail with a reuse error.
    let requires_dedup = match registry::find_spec(&cmd) {
        Some(spec) => class_requires_dedup(spec.class),
        None => false,
    };

    if !requires_dedup {
        return match state
            .invoke_dispatcher()
            .dispatch(identity.scopes.as_slice(), &cmd, &args)
            .await
        {
            Ok(outcome) => dispatch_outcome_response(outcome),
            Err(error) => invoke_error_response(error),
        };
    }

    // Fail closed: a router built without a dedup store must refuse mutating commands rather
    // than quietly dispatch them at-least-once.
    let Some(dedup) = state.dedup().cloned() else {
        tracing::error!(
            command = %cmd,
            "remote invoke reached a mutating command with no dedup store; refusing"
        );
        return invoke_error_response(RemoteInvokeError::internal(
            "The idempotency store is not configured; the request was not executed.",
        ));
    };

    match dedup
        .begin(&identity.device_id, &request_id, &cmd, &args)
        .await
    {
        Err(error) => return invoke_error_response(error),
        Ok(DedupDecision::Replay(record)) => return replay_response(record),
        Ok(DedupDecision::Proceed(reservation)) => {
            let dispatcher = state.invoke_dispatcher();
            let scopes = identity.scopes.as_slice().to_vec();
            let continuation = tokio::spawn(async move {
                let result = dispatcher.dispatch(&scopes, &cmd, &args).await;
                if let Ok(outcome) = &result {
                    reservation
                        .complete(&cmd, &args, recorded_outcome(outcome))
                        .await;
                }
                // On facade rejection or panic, the owned reservation drops without writing a
                // durable outcome. On handler cancellation this task remains alive and keeps the
                // reservation until dispatch and persistence terminate.
                result
            });

            match continuation.await {
                Ok(Ok(outcome)) => dispatch_outcome_response(outcome),
                Ok(Err(error)) => invoke_error_response(error),
                Err(error) => {
                    tracing::error!(
                        %error,
                        "remote invoke continuation failed before producing a response"
                    );
                    invoke_error_response(RemoteInvokeError::internal(
                        "The remote command failed before producing a response.",
                    ))
                }
            }
        }
    }
}

fn recorded_outcome(outcome: &DispatchOutcome) -> RecordedOutcome {
    let (kind, envelope) = match outcome {
        DispatchOutcome::Ok(result) => (
            RemoteDedupOutcomeKind::Ok,
            InvokeWireResponse::Ok {
                ok: true,
                result: result.clone(),
            },
        ),
        DispatchOutcome::Err(error) => (
            RemoteDedupOutcomeKind::Err,
            InvokeWireResponse::Err {
                ok: false,
                error: error.clone(),
            },
        ),
    };
    RecordedOutcome {
        kind,
        // The stored body is the exact envelope the client received, so a replay is
        // byte-identical rather than a re-derivation that could drift from this mapping.
        body: serde_json::to_string(&envelope).unwrap_or_else(|_| "null".to_string()),
    }
}

/// Replays a cached outcome verbatim, at the same 200 status the original response carried.
///
/// Both `ok` and `err` outcomes replay as HTTP 200: the transport succeeded in both cases, and
/// the client distinguishes them by the envelope's `ok` field exactly as it did the first time.
fn replay_response(record: RemoteRequestDedupRecord) -> axum::response::Response {
    match serde_json::from_str::<Value>(&record.response) {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(error) => {
            tracing::error!(
                request_id = %record.request_id,
                %error,
                "cached remote response is unreadable; refusing to re-execute"
            );
            // Deliberately NOT a re-dispatch: the record proves the command already ran.
            invoke_error_response(RemoteInvokeError::internal(
                "A cached outcome exists for this requestId but could not be replayed.",
            ))
        }
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
        ErrorCode::RemoteInvalidArguments => StatusCode::BAD_REQUEST,
        ErrorCode::RemoteInternalError => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
