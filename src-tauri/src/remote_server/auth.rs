//! Fail-closed bearer authentication for the remote listener (§4.4).
//!
//! Three properties this module exists to guarantee:
//!
//! 1. **Absent ≠ invalid ≠ store failure.** Each is a distinct [`RemoteAuthRejection`]
//!    variant with its own status. A repository or query error answers **500**, never a 401
//!    that would make a store outage indistinguishable from "this token is not paired"
//!    (stateful-workflow "fail closed on reads").
//! 2. **No bootstrap exception.** Unlike `require_admin_key`'s zero-keys pass
//!    (`api_keys.rs:69-83`), a host with zero paired devices still answers only the
//!    descriptor and `/pair` (A-2).
//! 3. **No trust headers.** Every inbound `X-RalphX-*` header is stripped before any handler
//!    runs, so a remounted :3847 handler that transitively consults `ProjectScope` sees
//!    `None`-scope semantics and no header-forgery path exists (§4.4).

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, HeaderName, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use ralphx_remote_protocol::{ErrorCode, Scope};

use crate::domain::entities::{RemoteAuditAction, RemoteDevice, RemoteDeviceId, RemoteScopeSet};
use crate::domain::repositories::{
    RemoteAuditLogRepository, RemoteDeviceLookup, RemoteDeviceRepository,
    RemotePairingCodeRepository, RemoteSessionRepository, RemoteWsTicketRepository,
};
use crate::domain::services::key_crypto::{generate_prefixed_key, hash_key};
use crate::infrastructure::sqlite::{DbConnection, SqliteRemoteAccessRepository};
use crate::remote_server::endpoints::RemoteRouterState;
use crate::remote_server::rate_limit::{auth_endpoint_key, RemoteRateLimitKey, RemoteRateLimiter};
use crate::remote_server::session_registry::RemoteSessionRegistry;
use crate::remote_server::settings::RemoteExposureMode;
use crate::remote_server::{remote_error_response, PRE_AUTH_ALLOWLIST, TICKET_AUTH_ALLOWLIST};

/// Device bearer tokens. Greppable and visually distinct from `rxk_live_` (:3848) keys.
pub(crate) const REMOTE_DEVICE_TOKEN_PREFIX: &str = "rxd_live_";
/// Pairing codes, shown once in the host UI and carried in the `ralphx://pair` URL hash.
pub(crate) const REMOTE_PAIRING_CODE_PREFIX: &str = "rxp_";
/// WS upgrade tickets.
pub(crate) const REMOTE_WS_TICKET_PREFIX: &str = "rxt_";

/// Pairing codes live 10 minutes (§4.2).
pub(crate) const PAIRING_CODE_TTL_SECS: i64 = 600;
/// WS tickets live 60 seconds (§3.1).
pub(crate) const WS_TICKET_TTL_SECS: i64 = 60;

/// Characters of a device token kept for display, e.g. `rxd_live_a3f2`.
const DEVICE_TOKEN_PREFIX_CHARS: usize = 13;
/// Cap on the audit `detail` column so a hostile URI cannot bloat the log.
const AUDIT_DETAIL_MAX_CHARS: usize = 200;

/// :3847 trust headers that must never be believed on :3849 (§4.4).
pub(crate) const STRIPPED_TRUST_HEADERS: &[&str] = &[
    "x-ralphx-external-mcp",
    "x-ralphx-key-id",
    "x-ralphx-project-scope",
    "x-ralphx-tauri-mcp",
];

/// Defense in depth: the whole vendor header namespace is dropped, not just the four names
/// that exist today, so a future :3847 trust header is stripped the day it is added.
pub(crate) const RALPHX_HEADER_NAMESPACE: &str = "x-ralphx-";

/// Fixed-width RFC3339 UTC, so lexicographic order over stored timestamps is chronological
/// order and expiry comparisons can run inside SQL or Rust interchangeably.
pub(crate) fn remote_timestamp(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

pub(crate) fn now_timestamp() -> String {
    remote_timestamp(Utc::now())
}

pub(crate) fn expiry_timestamp(from: DateTime<Utc>, ttl_secs: i64) -> String {
    remote_timestamp(from + ChronoDuration::seconds(ttl_secs))
}

/// Mints a raw device token. Returned to the client exactly once; only its hash is stored.
pub(crate) fn generate_device_token() -> String {
    generate_prefixed_key(REMOTE_DEVICE_TOKEN_PREFIX)
}

pub(crate) fn generate_pairing_code() -> String {
    generate_prefixed_key(REMOTE_PAIRING_CODE_PREFIX)
}

pub(crate) fn generate_ws_ticket() -> String {
    generate_prefixed_key(REMOTE_WS_TICKET_PREFIX)
}

/// Display prefix persisted alongside the hash, e.g. `rxd_live_a3f2`.
pub(crate) fn device_token_prefix(raw_token: &str) -> String {
    raw_token.chars().take(DEVICE_TOKEN_PREFIX_CHARS).collect()
}

/// The authenticated caller, attached to the request for downstream handlers.
///
/// Scopes are the ones read from the device row **on this request**, so a host-side toggle
/// takes effect on the next call without re-pairing (§3.1 P-28).
#[derive(Debug, Clone)]
pub(crate) struct RemoteIdentity {
    pub device_id: RemoteDeviceId,
    pub device_name: String,
    pub scopes: RemoteScopeSet,
}

impl RemoteIdentity {
    pub(crate) fn has_scope(&self, scope: Scope) -> bool {
        self.scopes.contains(scope)
    }

    pub(crate) fn agent_control_granted(&self) -> bool {
        self.has_scope(Scope::UiAgent)
    }
}

impl From<&RemoteDevice> for RemoteIdentity {
    fn from(device: &RemoteDevice) -> Self {
        Self {
            device_id: device.id.clone(),
            device_name: device.name.clone(),
            scopes: device.scopes.clone(),
        }
    }
}

/// Every way a remote request can be refused, each with its own status.
///
/// The split between [`Self::UnknownToken`] and [`Self::StoreUnavailable`] is the point of
/// this enum: collapsing them would let a database outage read as "not paired".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RemoteAuthRejection {
    #[error("no bearer token was presented")]
    MissingBearer,
    #[error("the Authorization header is not a well-formed bearer token")]
    MalformedBearer,
    #[error("the presented device token is not paired with this host")]
    UnknownToken,
    #[error("the presented device token has been revoked")]
    RevokedToken,
    #[error("this device does not hold the required scope")]
    InsufficientScope(Scope),
    #[error("the remote device store is unavailable: {0}")]
    StoreUnavailable(String),
    #[error("too many requests")]
    RateLimited { retry_after_secs: u64 },
    #[error("this device has too many requests in flight")]
    TooManyConcurrentRequests,
}

impl RemoteAuthRejection {
    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::MissingBearer
            | Self::MalformedBearer
            | Self::UnknownToken
            | Self::RevokedToken => StatusCode::UNAUTHORIZED,
            Self::InsufficientScope(_) => StatusCode::FORBIDDEN,
            // A store failure is the host's fault, not the caller's. Answering 401 here
            // would let an outage masquerade as a credential problem and would invite a
            // client to discard a perfectly good token.
            Self::StoreUnavailable(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::RateLimited { .. } | Self::TooManyConcurrentRequests => {
                StatusCode::TOO_MANY_REQUESTS
            }
        }
    }

    /// Wire code. The v1 protocol vocabulary (PR 0.2, snapshot-pinned) has no rate-limit or
    /// internal-error member, so those reuse the closest existing codes and rely on the HTTP
    /// status plus `Retry-After` to carry the distinction.
    pub(crate) fn error_code(&self) -> ErrorCode {
        match self {
            Self::MissingBearer
            | Self::MalformedBearer
            | Self::UnknownToken
            | Self::RevokedToken => ErrorCode::RemoteUnauthorized,
            Self::InsufficientScope(_) => ErrorCode::RemoteForbidden,
            Self::StoreUnavailable(_) => ErrorCode::RemoteUnreachable,
            Self::RateLimited { .. } | Self::TooManyConcurrentRequests => {
                ErrorCode::RemoteForbidden
            }
        }
    }

    pub(crate) fn audit_action(&self) -> RemoteAuditAction {
        match self {
            Self::StoreUnavailable(_) => RemoteAuditAction::AuthStoreError,
            Self::RateLimited { .. } | Self::TooManyConcurrentRequests => {
                RemoteAuditAction::RateLimited
            }
            _ => RemoteAuditAction::AuthRejected,
        }
    }

    /// Whether this rejection counts toward the auth-failure lockout.
    ///
    /// Store failures and rate limits deliberately do not: a flaky database must not lock the
    /// owner out, and counting a rate-limited request would compound the penalty.
    pub(crate) fn counts_as_auth_failure(&self) -> bool {
        matches!(
            self,
            Self::MalformedBearer | Self::UnknownToken | Self::RevokedToken
        )
    }

    fn retry_after_secs(&self) -> Option<u64> {
        match self {
            Self::RateLimited { retry_after_secs } => Some(*retry_after_secs),
            Self::TooManyConcurrentRequests => Some(1),
            _ => None,
        }
    }

    /// Client-facing message. Deliberately uniform across absent/unknown/revoked so the
    /// response cannot be used to probe which tokens exist.
    fn message(&self) -> String {
        match self {
            Self::MissingBearer
            | Self::MalformedBearer
            | Self::UnknownToken
            | Self::RevokedToken => "Remote authentication is required.".to_string(),
            Self::InsufficientScope(scope) => {
                format!("This device is not granted {}.", scope_label(*scope))
            }
            Self::StoreUnavailable(_) => {
                "The host could not verify this device right now.".to_string()
            }
            Self::RateLimited { .. } | Self::TooManyConcurrentRequests => {
                "Too many remote requests. Retry shortly.".to_string()
            }
        }
    }

    pub(crate) fn into_response(self) -> Response {
        let retry_after = self.retry_after_secs();
        let mut response = remote_error_response(self.status(), self.error_code(), self.message());
        if let Some(retry_after) = retry_after {
            if let Ok(value) = retry_after.to_string().parse() {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }
        response
    }
}

pub(crate) fn scope_label(scope: Scope) -> &'static str {
    match scope {
        Scope::UiRead => "ui:read",
        Scope::UiOperate => "ui:operate",
        Scope::UiAgent => "ui:agent",
        Scope::UiElevated => "ui:elevated",
    }
}

/// Everything the remote router needs to authenticate, authorize, and audit.
#[derive(Clone)]
pub(crate) struct RemoteAuthContext {
    pub devices: Arc<dyn RemoteDeviceRepository>,
    pub pairing_codes: Arc<dyn RemotePairingCodeRepository>,
    pub sessions: Arc<dyn RemoteSessionRepository>,
    pub tickets: Arc<dyn RemoteWsTicketRepository>,
    pub audit: Arc<dyn RemoteAuditLogRepository>,
    pub registry: RemoteSessionRegistry,
    pub limiter: RemoteRateLimiter,
    pub exposure_mode: RemoteExposureMode,
}

impl RemoteAuthContext {
    /// Builds the context from one SQLite store shared across all five repository roles.
    pub(crate) fn from_db(
        db: DbConnection,
        registry: RemoteSessionRegistry,
        exposure_mode: RemoteExposureMode,
    ) -> Self {
        let store = Arc::new(SqliteRemoteAccessRepository::from_db(db));
        Self {
            devices: store.clone(),
            pairing_codes: store.clone(),
            sessions: store.clone(),
            tickets: store.clone(),
            audit: store,
            registry,
            limiter: RemoteRateLimiter::default(),
            exposure_mode,
        }
    }

    /// Host-local view of the same stores and the same live-session registry.
    ///
    /// The exposure mode only steers request-path rate-limit keying, which host-local Tauri
    /// commands never traverse, so `Serve` is a safe placeholder here. What must be shared is
    /// the registry: a revoke command that killed channels in a private copy would report
    /// success while the real sessions stayed live.
    pub(crate) fn host_local(db: DbConnection, registry: RemoteSessionRegistry) -> Self {
        Self::from_db(db, registry, RemoteExposureMode::Serve)
    }

    /// Writes an audit row, returning whether it landed.
    ///
    /// Callers granting access must treat `false` as fatal: authorizing a request whose
    /// decision left no trail is exactly the silent-failure this log exists to prevent.
    pub(crate) async fn record_audit(
        &self,
        device_id: Option<&RemoteDeviceId>,
        action: RemoteAuditAction,
        detail: Option<&str>,
    ) -> bool {
        let truncated = detail.map(truncate_detail);
        match self
            .audit
            .record(device_id, action, truncated.as_deref(), &now_timestamp())
            .await
        {
            Ok(()) => true,
            Err(error) => {
                tracing::error!(%error, ?action, "Remote audit log write failed");
                false
            }
        }
    }

    /// Resolves a raw bearer token to a live device.
    pub(crate) async fn resolve_device(
        &self,
        raw_token: &str,
    ) -> Result<RemoteDevice, RemoteAuthRejection> {
        match self
            .devices
            .lookup_by_token_hash(&hash_key(raw_token))
            .await
        {
            Ok(RemoteDeviceLookup::Active(device)) => Ok(device),
            Ok(RemoteDeviceLookup::Revoked(_)) => Err(RemoteAuthRejection::RevokedToken),
            Ok(RemoteDeviceLookup::Unknown) => Err(RemoteAuthRejection::UnknownToken),
            Err(error) => Err(RemoteAuthRejection::StoreUnavailable(error.to_string())),
        }
    }

    /// Tears a device's live sessions down and closes their durable rows.
    ///
    /// Order is fixed: durable authority (`revoked_at` / narrowed scopes) must already be
    /// written by the caller; this is only the effect (§4.4).
    pub(crate) async fn tear_down_device_sessions(
        &self,
        device_id: &RemoteDeviceId,
        reason: ralphx_remote_protocol::ResetReason,
    ) -> usize {
        let signalled = self.registry.kill_device(device_id, reason);
        let now = now_timestamp();
        if let Err(error) = self.sessions.close_all_for_device(device_id, &now).await {
            tracing::error!(%error, %device_id, "Closing remote session rows failed");
        }
        if let Err(error) = self.tickets.consume_all_for_device(device_id, &now).await {
            tracing::error!(%error, %device_id, "Invalidating remote ws tickets failed");
        }
        signalled
    }
}

fn truncate_detail(detail: &str) -> String {
    detail.chars().take(AUDIT_DETAIL_MAX_CHARS).collect()
}

/// Extracts the raw bearer token, distinguishing "absent" from "malformed".
pub(crate) fn extract_bearer(headers: &HeaderMap) -> Result<String, RemoteAuthRejection> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Err(RemoteAuthRejection::MissingBearer);
    };
    let value = value
        .to_str()
        .map_err(|_| RemoteAuthRejection::MalformedBearer)?;
    let (scheme, token) = value
        .split_once(' ')
        .ok_or(RemoteAuthRejection::MalformedBearer)?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(RemoteAuthRejection::MalformedBearer);
    }
    let token = token.trim();
    if token.is_empty() {
        return Err(RemoteAuthRejection::MalformedBearer);
    }
    Ok(token.to_string())
}

/// Enforces a scope on an authenticated identity.
pub(crate) fn require_scope(
    identity: &RemoteIdentity,
    scope: Scope,
) -> Result<(), RemoteAuthRejection> {
    if identity.has_scope(scope) {
        Ok(())
    } else {
        Err(RemoteAuthRejection::InsufficientScope(scope))
    }
}

fn peer_address(request: &Request) -> Option<IpAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip())
}

fn request_detail(request: &Request) -> String {
    format!("{} {}", request.method(), request.uri().path())
}

// ---------------------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------------------

/// Drops every inbound `X-RalphX-*` header before any handler or extractor sees it.
///
/// Outermost layer on the remote router: even the pre-auth allowlisted routes run behind it.
pub(crate) async fn strip_trust_headers(mut request: Request, next: Next) -> Response {
    let headers = request.headers_mut();
    let forged: Vec<HeaderName> = headers
        .keys()
        .filter(|name| name.as_str().starts_with(RALPHX_HEADER_NAMESPACE))
        .cloned()
        .collect();
    for name in forged {
        headers.remove(&name);
    }
    for name in STRIPPED_TRUST_HEADERS {
        if let Ok(name) = HeaderName::from_bytes(name.as_bytes()) {
            headers.remove(&name);
        }
    }
    next.run(request).await
}

/// Token bucket + lockout ahead of `/remote/v1/auth/*` AND the WS upgrade, before any body
/// is read.
///
/// The identity is the Serve-aware pre-auth key; the per-pairing-code lockout that keeps two
/// devices independent is applied inside the pair handler, where the code is known.
///
/// The WS upgrade is an auth endpoint in everything but its path: it redeems a single-use
/// credential and answers a DB-backed accept/reject. Leaving it out let an unauthenticated
/// tailnet peer drive unbounded ticket guesses, each one a store round-trip and a durable
/// `remote_audit_log` row (§4.4, §4.6).
pub(crate) async fn enforce_auth_endpoint_rate_limit(
    State(state): State<RemoteRouterState>,
    request: Request,
    next: Next,
) -> Response {
    // Preflight is pre-auth by contract (C-15) and carries no credential to brute-force, so
    // it must not spend the auth budget.
    let path = request.uri().path();
    let rate_limited = path.starts_with("/remote/v1/auth/") || path == super::ws::WS_EVENTS_PATH;
    if request.method() == axum::http::Method::OPTIONS || !rate_limited {
        return next.run(request).await;
    }
    let key = auth_endpoint_key(state.auth().exposure_mode, peer_address(&request));
    let decision = state.auth().limiter.check(&key);
    if decision.is_allowed() {
        return next.run(request).await;
    }
    let retry_after_secs = decision.retry_after_secs().unwrap_or(1);
    // Deliberately no audit row: this rejection carries no identity, and a flood that trips
    // the bucket would otherwise append one durable row per refused request — letting refused
    // traffic out-write accepted traffic. Attributable rate limiting (per device, per pairing
    // code) still audits, because it is bounded by the identity it names.
    tracing::warn!(
        detail = %request_detail(&request),
        retry_after_secs,
        "Remote auth endpoint refused a request over the pre-auth flood ceiling"
    );
    RemoteAuthRejection::RateLimited { retry_after_secs }.into_response()
}

/// The global fail-closed bearer check.
///
/// Preflight and the two pre-auth routes pass through unchanged; everything else must
/// present a live device token, hold a per-device rate-limit token and concurrency slot, and
/// leave an audit row before the handler runs.
pub(crate) async fn authenticate_remote_request(
    State(state): State<RemoteRouterState>,
    mut request: Request,
    next: Next,
) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    if PRE_AUTH_ALLOWLIST.contains(&request.uri().path()) {
        return next.run(request).await;
    }
    // The WS upgrade carries no bearer by construction (§3.2) and authenticates itself with a
    // single-use device-bound ticket before upgrading. Skipping the bearer check here is not a
    // hole: `ws_events_handler` refuses every request that cannot consume a live ticket AND
    // re-resolve an unrevoked device holding `ui:read`.
    if TICKET_AUTH_ALLOWLIST.contains(&request.uri().path()) {
        return next.run(request).await;
    }

    let auth = state.auth().clone();
    let detail = request_detail(&request);
    let peer_key = auth_endpoint_key(auth.exposure_mode, peer_address(&request));

    let raw_token = match extract_bearer(request.headers()) {
        Ok(token) => token,
        // No token was presented, so there is no identity to punish: rate limiting alone
        // governs this path (a lockout would have to land on a shared key).
        Err(rejection) => return reject(&auth, rejection, None, &detail, None, &peer_key).await,
    };
    // The failure identity is the presented token, never the shared pre-auth key: a bad
    // bearer must not be able to lock out pairing or ws-ticket minting for other devices.
    let token_key = RemoteRateLimitKey::bearer_token(hash_key(&raw_token));
    let device = match auth.resolve_device(&raw_token).await {
        Ok(device) => device,
        Err(rejection) => {
            return reject(&auth, rejection, None, &detail, Some(&token_key), &peer_key).await
        }
    };

    let device_key = RemoteRateLimitKey::device(&device.id);
    if let Some(retry_after_secs) = auth.limiter.check(&device_key).retry_after_secs() {
        return reject(
            &auth,
            RemoteAuthRejection::RateLimited { retry_after_secs },
            Some(&device.id),
            &detail,
            None,
            &peer_key,
        )
        .await;
    }
    let Some(_slot) = auth.limiter.acquire_device_slot(&device.id) else {
        return reject(
            &auth,
            RemoteAuthRejection::TooManyConcurrentRequests,
            Some(&device.id),
            &detail,
            None,
            &peer_key,
        )
        .await;
    };

    // Authority is established; record it before the handler can produce any effect. A
    // failed audit write refuses the request rather than granting untraceable access.
    if !auth
        .record_audit(
            Some(&device.id),
            RemoteAuditAction::AuthAccepted,
            Some(&detail),
        )
        .await
    {
        return RemoteAuthRejection::StoreUnavailable("audit log write failed".to_string())
            .into_response();
    }
    if let Err(error) = auth
        .devices
        .touch_last_seen(&device.id, &now_timestamp())
        .await
    {
        tracing::warn!(%error, device_id = %device.id, "Updating remote device last_seen_at failed");
    }
    auth.limiter.record_success(&token_key);

    request
        .extensions_mut()
        .insert(RemoteIdentity::from(&device));
    next.run(request).await
}

/// Refuses a request, optionally charging the failure to `failure_key`.
///
/// `failure_key` is `None` whenever no attributable credential was presented (absent or
/// malformed bearer, rate limiting). It is never the pre-auth key: charging the lockout
/// there would let one peer deny pairing and ws-ticket minting for every device (§4.4, P-8).
/// `peer_key` is used only as a coarse bucket, to bound how fast an unauthenticated flood can
/// append to the audit log.
async fn reject(
    auth: &RemoteAuthContext,
    rejection: RemoteAuthRejection,
    device_id: Option<&RemoteDeviceId>,
    detail: &str,
    failure_key: Option<&RemoteRateLimitKey>,
    peer_key: &RemoteRateLimitKey,
) -> Response {
    if rejection.counts_as_auth_failure() {
        if let Some(failure_key) = failure_key {
            auth.limiter.record_failure(failure_key);
        }
    }
    // Authenticated rejections always leave a trail. Unauthenticated ones are the only path a
    // flooder can drive without limit, and every audit row is a durable write, so they spend
    // the coarse pre-auth bucket: past the ceiling the request is still refused, it just stops
    // appending a row per attempt.
    if device_id.is_some() || auth.limiter.check(peer_key).is_allowed() {
        let audit_detail = format!("{detail} — {rejection}");
        auth.record_audit(device_id, rejection.audit_action(), Some(&audit_detail))
            .await;
    }
    tracing::debug!(%rejection, detail, "Remote request refused");
    rejection.into_response()
}
