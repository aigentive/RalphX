//! The three authenticated-surface endpoints PR 1.2 owns: pairing, WS tickets, and session
//! introspection/teardown (§3.1).
//!
//! `/remote/v1/auth/pair` is one of exactly two pre-auth routes; it is the only place a
//! device credential is ever minted, and the raw token appears exactly once — in the
//! response body, consumed by the client's Rust backend and stored in the Keychain (§4.2).

use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use ralphx_remote_protocol::{ErrorCode, ResetReason, Scope, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};

use crate::domain::entities::{RemoteAuditAction, RemoteDeviceId, RemoteScopeSet, RemoteSessionId};
use crate::domain::repositories::RemotePairingOutcome;
use crate::domain::services::key_crypto::hash_key;
use crate::remote_server::auth::{
    device_token_prefix, expiry_timestamp, generate_device_token, generate_ws_ticket,
    now_timestamp, require_scope, RemoteAuthRejection, RemoteIdentity, WS_TICKET_TTL_SECS,
};
use crate::remote_server::endpoints::RemoteRouterState;
use crate::remote_server::rate_limit::RemoteRateLimitKey;
use crate::remote_server::remote_error_response;
use crate::remote_server::ws::SessionLifecycleSink;

/// Local-only event: a pairing code was redeemed and a device now exists (§5.5).
///
/// Pairing happens over HTTP on the remote listener, in a different process path from the
/// Tauri UI, so nothing else tells the host's own Remote Access pane that its device list
/// changed. This is the durable-authority signal for that: the pane re-reads the backend on
/// it rather than inferring the new device from anything it holds locally.
///
/// **Local-only is a security property here, exactly as it is for the session events** —
/// forwarding it would tell every paired device when another device joins. The capture bank
/// drops Local-only rows structurally, so it can never reach the sequencer or the durable log.
pub(crate) const REMOTE_DEVICE_PAIRED_EVENT: &str = "remote:device_paired";

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelfRevokeResponse {
    pub device_id: String,
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
        audit_detail: Some(format!(
            "{} ({})",
            device_token_prefix(&raw_token),
            request
                .client_version
                .as_deref()
                .unwrap_or("unknown client")
        )),
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
            // The `pairing_succeeded` audit row committed with the device inside `redeem`;
            // there is deliberately no post-commit audit write here, because failing one
            // would strand an active device whose token was never handed to anybody.
            tracing::info!(device_id = %device.id, "Remote device paired");
            // Emitted only on the committed `Paired` outcome, after `redeem` accepted the
            // transition — the pane treats this as "re-read the backend", never as the new
            // device's contents, so a rejected or errored pairing can never move its lists.
            let lifecycle = state.lifecycle();
            lifecycle.emit(
                REMOTE_DEVICE_PAIRED_EVENT,
                serde_json::json!({
                    "deviceId": device.id.to_string(),
                    "deviceName": device.name,
                }),
            );
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

/// Kills the caller's **own** device credential, not just its sessions.
///
/// The client's staged remove/re-pair machines are specified as "host revoke (best-effort) →
/// Keychain delete → row delete" (§6.1, P-27), which requires a host surface a client can
/// actually call — device management itself stays host-local Tauri (§3.1/§5.4), so this is
/// deliberately self-scoped: the identity comes from the bearer the middleware already
/// resolved, never from the body, so no device can revoke another.
///
/// Order matches `revoke_remote_device` (§4.4): durable `revoked_at` first, then the audit
/// row, then the live-session teardown. A repository failure answers 500 so the client keeps
/// the token referenced and retries on the next reconcile instead of orphaning a live bearer.
pub(crate) async fn self_revoke_handler(
    State(state): State<RemoteRouterState>,
    Extension(identity): Extension<RemoteIdentity>,
) -> Response {
    let auth = state.auth().clone();
    let revoked = match auth
        .devices
        .revoke(&identity.device_id, &now_timestamp())
        .await
    {
        Ok(Some(device)) => device,
        // The row vanished between the middleware's lookup and this write: the token is
        // already unusable, which is exactly what the caller asked for.
        Ok(None) => return RemoteAuthRejection::UnknownToken.into_response(),
        Err(error) => {
            return RemoteAuthRejection::StoreUnavailable(error.to_string()).into_response()
        }
    };
    auth.record_audit(
        Some(&revoked.id),
        RemoteAuditAction::DeviceRevoked,
        Some("client requested self-revocation"),
    )
    .await;
    let closed_sessions = auth
        .tear_down_device_sessions(&revoked.id, ResetReason::Revoked)
        .await;
    tracing::info!(device_id = %revoked.id, sessions = closed_sessions, "Remote device self-revoked");

    (
        StatusCode::OK,
        Json(SelfRevokeResponse {
            device_id: revoked.id.to_string(),
            closed_sessions,
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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionTeardownQuery {
    /// The caller's own WS session. Absent over plain HTTP, where the caller has no session
    /// identity beyond its device.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Ends the caller's own live sessions without revoking the device.
///
/// Narrowed in PR 1.4: when the caller names a `sessionId` it owns, only that socket is torn
/// down. Closing every session of a device that merely has several tabs open is a bigger effect
/// than "log this one out" ever asked for. The all-close form remains the fallback for plain HTTP
/// callers with no session identity.
///
/// The named session is checked against the **registry's live list for this device**, so naming
/// another device's session id closes nothing — the scoping is by ownership, not by trust in the
/// client-supplied id.
///
/// The reset reason is `Revoked` only because the pinned v1 vocabulary has no "session
/// closed on request" member — the credential is untouched. A client that receives it after
/// asking for teardown (or after an agent-control narrowing, `remote_device_commands.rs`)
/// must reconnect and re-introspect (P-28) rather than treating its token as dead;
/// `POST /remote/v1/auth/revoke` is the only path that actually kills the credential.
pub(crate) async fn session_teardown_handler(
    State(state): State<RemoteRouterState>,
    Extension(identity): Extension<RemoteIdentity>,
    Query(query): Query<SessionTeardownQuery>,
) -> Response {
    let auth = state.auth().clone();

    if let Some(session_id) = query
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        let session_id = RemoteSessionId::from_string(session_id);
        let owns_session = auth
            .registry
            .live_sessions(&identity.device_id)
            .contains(&session_id);
        if !owns_session {
            auth.record_audit(
                Some(&identity.device_id),
                RemoteAuditAction::AuthRejected,
                Some("session teardown named a session this device does not hold"),
            )
            .await;
            return remote_error_response(
                StatusCode::NOT_FOUND,
                ErrorCode::RemoteCommandUnavailable,
                "This device holds no such live session.",
            );
        }

        let closed = usize::from(auth.registry.kill_session(
            &identity.device_id,
            &session_id,
            ResetReason::Revoked,
        ));
        if let Err(error) = auth.sessions.close(&session_id, &now_timestamp()).await {
            tracing::warn!(%error, %session_id, "Closing the remote session row failed");
        }
        auth.record_audit(
            Some(&identity.device_id),
            RemoteAuditAction::SessionClosed,
            Some("client requested teardown of its own session"),
        )
        .await;
        return (
            StatusCode::OK,
            Json(SessionTeardownResponse {
                closed_sessions: closed,
            }),
        )
            .into_response();
    }

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
