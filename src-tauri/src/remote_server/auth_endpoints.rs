//! The three authenticated-surface endpoints PR 1.2 owns: pairing, WS tickets, and session
//! introspection/teardown (§3.1).
//!
//! `/remote/v1/auth/pair` is one of exactly two pre-auth routes; it is the only place a
//! device credential is ever minted, and the raw token appears exactly once — in the
//! response body, consumed by the client's Rust backend and stored in the Keychain (§4.2).

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use ralphx_remote_protocol::{ErrorCode, ResetReason, Scope, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};

use crate::domain::entities::{RemoteAuditAction, RemoteDeviceId, RemoteScopeSet};
use crate::domain::repositories::RemotePairingOutcome;
use crate::domain::services::key_crypto::hash_key;
use crate::remote_server::auth::{
    device_token_prefix, expiry_timestamp, generate_device_token, generate_ws_ticket,
    now_timestamp, require_scope, RemoteAuthRejection, RemoteIdentity, WS_TICKET_TTL_SECS,
};
use crate::remote_server::endpoints::RemoteRouterState;
use crate::remote_server::rate_limit::RemoteRateLimitKey;
use crate::remote_server::remote_error_response;

/// Longest device name the host will store, so a hostile client cannot bloat the row.
const MAX_DEVICE_NAME_CHARS: usize = 120;
/// Body budget for the auth endpoints; a pairing request is a few hundred bytes.
pub(crate) const REMOTE_AUTH_BODY_LIMIT_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PairRequest {
    pub pairing_code: String,
    pub device_name: String,
    #[serde(default)]
    pub client_version: Option<String>,
    /// Must be a subset of the code's grant; absent takes the whole grant (§4.2).
    #[serde(default)]
    pub requested_scopes: Option<Vec<Scope>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PairResponse {
    /// The only time the raw token exists outside the client's Keychain.
    pub device_token: String,
    pub device_id: String,
    pub scopes: Vec<Scope>,
    pub environment_id: String,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WsTicketResponse {
    pub ticket: String,
    pub expires_in_secs: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionIntrospection {
    pub device_id: String,
    pub device_name: String,
    /// The **currently effective** grant, re-read on every request — a host-side toggle
    /// lands here without the client re-pairing (P-28).
    pub scopes: Vec<Scope>,
    pub agent_control_granted: bool,
    pub environment_id: String,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionTeardownResponse {
    pub closed_sessions: usize,
}

/// Exchanges a single-use pairing code for a device token.
///
/// Rate limiting here keys on the **presented code**, not the socket: under Serve every
/// caller is loopback, so a per-IP lockout would let one peer lock every device out (§4.4).
pub(crate) async fn pair_handler(
    State(state): State<RemoteRouterState>,
    Json(request): Json<PairRequest>,
) -> Response {
    let auth = state.auth().clone();
    let code_key = RemoteRateLimitKey::pairing_code(hash_key(&request.pairing_code));

    if let Some(retry_after_secs) = auth.limiter.check(&code_key).retry_after_secs() {
        auth.record_audit(
            None,
            RemoteAuditAction::RateLimited,
            Some("POST /remote/v1/auth/pair"),
        )
        .await;
        return RemoteAuthRejection::RateLimited { retry_after_secs }.into_response();
    }

    let device_name = request.device_name.trim();
    if device_name.is_empty() || device_name.chars().count() > MAX_DEVICE_NAME_CHARS {
        auth.limiter.record_failure(&code_key);
        auth.record_audit(
            None,
            RemoteAuditAction::PairingRejected,
            Some("device name is missing or too long"),
        )
        .await;
        return remote_error_response(
            StatusCode::BAD_REQUEST,
            ErrorCode::RemoteForbidden,
            "A device name is required.",
        );
    }

    let raw_token = generate_device_token();
    let redemption = crate::domain::repositories::RemotePairingRedemption {
        code_hash: hash_key(&request.pairing_code),
        device_id: RemoteDeviceId::new(),
        device_name: device_name.to_string(),
        token_hash: hash_key(&raw_token),
        token_prefix: device_token_prefix(&raw_token),
        requested_scopes: request
            .requested_scopes
            .clone()
            .map(|scopes| RemoteScopeSet::from_scopes(scopes)),
        now: now_timestamp(),
    };

    let outcome = match auth.pairing_codes.redeem(redemption).await {
        Ok(outcome) => outcome,
        Err(error) => {
            // A store failure is not a pairing refusal: it must not consume the code's
            // failure budget and must not read to the client as a bad code.
            auth.record_audit(
                None,
                RemoteAuditAction::AuthStoreError,
                Some(&format!("pair: {error}")),
            )
            .await;
            return RemoteAuthRejection::StoreUnavailable(error.to_string()).into_response();
        }
    };

    match outcome {
        RemotePairingOutcome::Paired(device) => {
            auth.limiter.record_success(&code_key);
            if !auth
                .record_audit(
                    Some(&device.id),
                    RemoteAuditAction::PairingSucceeded,
                    Some(&format!(
                        "{} ({})",
                        device.token_prefix,
                        request
                            .client_version
                            .as_deref()
                            .unwrap_or("unknown client")
                    )),
                )
                .await
            {
                return RemoteAuthRejection::StoreUnavailable("audit log write failed".to_string())
                    .into_response();
            }
            tracing::info!(device_id = %device.id, "Remote device paired");
            (
                StatusCode::OK,
                Json(PairResponse {
                    device_token: raw_token,
                    device_id: device.id.to_string(),
                    scopes: device.scopes.to_vec(),
                    environment_id: state.environment_id().to_string(),
                    protocol_version: PROTOCOL_VERSION,
                }),
            )
                .into_response()
        }
        RemotePairingOutcome::ScopeNotGranted(scope) => {
            auth.limiter.record_failure(&code_key);
            auth.record_audit(
                None,
                RemoteAuditAction::PairingRejected,
                Some("requested scopes exceed the pairing grant"),
            )
            .await;
            RemoteAuthRejection::InsufficientScope(scope).into_response()
        }
        rejected => {
            auth.limiter.record_failure(&code_key);
            auth.record_audit(
                None,
                RemoteAuditAction::PairingRejected,
                Some(&format!("{rejected:?}")),
            )
            .await;
            // Uniform message: unknown, expired, and already-consumed must be
            // indistinguishable so the endpoint is not an oracle for outstanding codes.
            remote_error_response(
                StatusCode::UNAUTHORIZED,
                ErrorCode::RemoteUnauthorized,
                "This pairing code is not valid.",
            )
        }
    }
}

/// Issues a single-use, device-bound WS upgrade ticket (PR 1.4 consumes it).
pub(crate) async fn ws_ticket_handler(
    State(state): State<RemoteRouterState>,
    Extension(identity): Extension<RemoteIdentity>,
) -> Response {
    let auth = state.auth().clone();
    // The ticket buys access to the event stream, so it needs the read scope — no more.
    if let Err(rejection) = require_scope(&identity, Scope::UiRead) {
        auth.record_audit(
            Some(&identity.device_id),
            RemoteAuditAction::WsTicketRejected,
            Some("ui:read is not granted"),
        )
        .await;
        return rejection.into_response();
    }

    let raw_ticket = generate_ws_ticket();
    let expires_at = expiry_timestamp(Utc::now(), WS_TICKET_TTL_SECS);
    if let Err(error) = auth
        .tickets
        .issue(&hash_key(&raw_ticket), &identity.device_id, &expires_at)
        .await
    {
        auth.record_audit(
            Some(&identity.device_id),
            RemoteAuditAction::AuthStoreError,
            Some(&format!("ws-ticket: {error}")),
        )
        .await;
        return RemoteAuthRejection::StoreUnavailable(error.to_string()).into_response();
    }
    auth.record_audit(
        Some(&identity.device_id),
        RemoteAuditAction::WsTicketIssued,
        None,
    )
    .await;

    (
        StatusCode::OK,
        Json(WsTicketResponse {
            ticket: raw_ticket,
            expires_in_secs: WS_TICKET_TTL_SECS,
        }),
    )
        .into_response()
}

/// Reports the caller's currently effective grant.
///
/// Built from the identity the middleware resolved on **this** request, so it reflects
/// host-side toggles immediately and never replays the pair-time mint (P-28).
pub(crate) async fn session_introspection_handler(
    State(state): State<RemoteRouterState>,
    Extension(identity): Extension<RemoteIdentity>,
) -> Response {
    (
        StatusCode::OK,
        Json(SessionIntrospection {
            device_id: identity.device_id.to_string(),
            device_name: identity.device_name.clone(),
            scopes: identity.scopes.to_vec(),
            agent_control_granted: identity.agent_control_granted(),
            environment_id: state.environment_id().to_string(),
            protocol_version: PROTOCOL_VERSION,
        }),
    )
        .into_response()
}

/// Ends the caller's own live sessions without revoking the device.
///
/// PR 1.4 narrows this to the calling WS session once a session id exists at request time;
/// over plain HTTP the caller has no session identity beyond its device.
pub(crate) async fn session_teardown_handler(
    State(state): State<RemoteRouterState>,
    Extension(identity): Extension<RemoteIdentity>,
) -> Response {
    let auth = state.auth().clone();
    let closed = auth
        .tear_down_device_sessions(&identity.device_id, ResetReason::Revoked)
        .await;
    auth.record_audit(
        Some(&identity.device_id),
        RemoteAuditAction::SessionClosed,
        Some("client requested session teardown"),
    )
    .await;

    (
        StatusCode::OK,
        Json(SessionTeardownResponse {
            closed_sessions: closed,
        }),
    )
        .into_response()
}
