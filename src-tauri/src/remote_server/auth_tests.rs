use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
    middleware,
    response::Response,
    routing::get,
    Json, Router,
};
use ralphx_remote_protocol::{ResetReason, Scope};
use serde_json::{json, Value};
use tower::ServiceExt;

use super::auth::{
    device_token_prefix, expiry_timestamp, generate_device_token, generate_pairing_code,
    generate_ws_ticket, now_timestamp, scope_label, strip_trust_headers, RemoteAuthContext,
    RemoteAuthRejection, PAIRING_CODE_TTL_SECS, RALPHX_HEADER_NAMESPACE,
    REMOTE_DEVICE_TOKEN_PREFIX, REMOTE_PAIRING_CODE_PREFIX, REMOTE_WS_TICKET_PREFIX,
    STRIPPED_TRUST_HEADERS, WS_TICKET_TTL_SECS,
};
use super::endpoints::RemoteRouterState;
use super::session_registry::{RemoteSessionAdmission, RemoteSessionRegistry};
use super::settings::RemoteExposureMode;
use super::{
    authenticated_remote_routes, DESCRIPTOR_PATH, HEALTH_PATH, PAIR_PATH, REVOKE_PATH,
    SESSION_PATH, WS_TICKET_PATH,
};
use crate::domain::entities::{
    RemoteAuditAction, RemoteAuditEntry, RemoteDevice, RemoteDeviceId, RemotePairingCode,
    RemotePairingCodeId, RemoteScopeSet, RemoteSession, RemoteSessionId,
};
use crate::domain::repositories::{
    RemoteAuditLogRepository, RemoteDeviceLookup, RemoteDeviceRepository,
    RemotePairingCodeRepository, RemotePairingOutcome, RemotePairingRedemption,
    RemoteWsTicketOutcome,
};
use crate::domain::services::key_crypto::hash_key;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::{run_migrations, DbConnection};
use crate::remote_server::invoke::RemoteInvokeDispatcher;
use crate::remote_server::registry::{DispatchOutcome, RemoteInvokeError};

const TEST_ENVIRONMENT_ID: &str = "11111111-2222-3333-4444-555555555555";

/// A migrated in-memory store plus a fresh registry — enough to serve the whole remote
/// router without touching the filesystem.
pub(super) fn in_memory_auth_context() -> RemoteAuthContext {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory database should open");
    run_migrations(&conn).expect("migrations should apply to the in-memory database");
    RemoteAuthContext::from_db(
        DbConnection::new(conn),
        RemoteSessionRegistry::new(),
        RemoteExposureMode::Serve,
    )
}

fn router_for(context: &RemoteAuthContext) -> Router {
    authenticated_remote_routes(RemoteRouterState::new_with_invoke_dispatcher(
        TEST_ENVIRONMENT_ID,
        context.clone(),
        Arc::new(UnavailableInvokeDispatcher),
    ))
}

struct UnavailableInvokeDispatcher;

#[async_trait]
impl RemoteInvokeDispatcher for UnavailableInvokeDispatcher {
    async fn dispatch(
        &self,
        _scopes: &[Scope],
        _command: &str,
        _args: &Value,
    ) -> Result<DispatchOutcome, RemoteInvokeError> {
        Err(RemoteInvokeError {
            code: ralphx_remote_protocol::ErrorCode::RemoteCommandUnavailable,
            message: "invoke is unavailable in auth router tests".to_string(),
        })
    }
}

async fn body_json(response: Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn get_with_bearer(path: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(Method::GET).uri(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::empty()).expect("request should build")
}

fn post_json(path: &str, token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder
        .body(Body::from(body.to_string()))
        .expect("request should build")
}

fn delete_with_bearer(path: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(Method::DELETE)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request should build")
}

/// Mints a pairing code the way `generate_remote_pairing_code` does.
async fn mint_pairing_code(context: &RemoteAuthContext, scopes: RemoteScopeSet) -> String {
    let raw = generate_pairing_code();
    context
        .pairing_codes
        .create(RemotePairingCode {
            id: RemotePairingCodeId::new(),
            code_hash: hash_key(&raw),
            scopes,
            created_at: now_timestamp(),
            expires_at: expiry_timestamp(chrono::Utc::now(), PAIRING_CODE_TTL_SECS),
            consumed_at: None,
        })
        .await
        .expect("pairing code should insert");
    raw
}

/// Pairs a device through the real HTTP surface and returns its raw token.
async fn pair_device(context: &RemoteAuthContext, name: &str) -> (String, RemoteDeviceId) {
    let code = mint_pairing_code(context, RemoteScopeSet::default_pairing_grant()).await;
    let response = router_for(context)
        .oneshot(post_json(
            PAIR_PATH,
            None,
            json!({"pairingCode": code, "deviceName": name, "clientVersion": "0.81.0"}),
        ))
        .await
        .expect("pair request should complete");
    assert_eq!(response.status(), StatusCode::OK, "pairing should succeed");
    let body = body_json(response).await;
    (
        body["deviceToken"]
            .as_str()
            .expect("a device token is returned")
            .to_string(),
        RemoteDeviceId::from_string(
            body["deviceId"]
                .as_str()
                .expect("a device id is returned")
                .to_string(),
        ),
    )
}

async fn audit_actions(context: &RemoteAuthContext) -> Vec<String> {
    context
        .audit
        .list_recent(Some(200))
        .await
        .expect("audit log should read")
        .into_iter()
        .map(|entry: RemoteAuditEntry| entry.action)
        .collect()
}

/// A device store whose every read fails — stands in for a locked or corrupted database.
struct FailingDeviceRepository;

#[async_trait]
impl RemoteDeviceRepository for FailingDeviceRepository {
    async fn lookup_by_token_hash(&self, _token_hash: &str) -> AppResult<RemoteDeviceLookup> {
        Err(AppError::Database("database is locked".to_string()))
    }

    async fn get(&self, _id: &RemoteDeviceId) -> AppResult<Option<RemoteDevice>> {
        Err(AppError::Database("database is locked".to_string()))
    }

    async fn list(&self) -> AppResult<Vec<RemoteDevice>> {
        Err(AppError::Database("database is locked".to_string()))
    }

    async fn revoke(&self, _id: &RemoteDeviceId, _now: &str) -> AppResult<Option<RemoteDevice>> {
        Err(AppError::Database("database is locked".to_string()))
    }

    async fn set_scopes(
        &self,
        _id: &RemoteDeviceId,
        _scopes: &RemoteScopeSet,
    ) -> AppResult<Option<RemoteDevice>> {
        Err(AppError::Database("database is locked".to_string()))
    }

    async fn touch_last_seen(&self, _id: &RemoteDeviceId, _now: &str) -> AppResult<()> {
        Err(AppError::Database("database is locked".to_string()))
    }
}

/// A pairing-code store whose every call fails.
struct FailingPairingCodeRepository;

#[async_trait]
impl RemotePairingCodeRepository for FailingPairingCodeRepository {
    async fn create(&self, _code: RemotePairingCode) -> AppResult<RemotePairingCode> {
        Err(AppError::Database("database is locked".to_string()))
    }

    async fn redeem(
        &self,
        _redemption: RemotePairingRedemption,
    ) -> AppResult<RemotePairingOutcome> {
        Err(AppError::Database("database is locked".to_string()))
    }

    async fn list_outstanding(&self, _now: &str) -> AppResult<Vec<RemotePairingCode>> {
        Err(AppError::Database("database is locked".to_string()))
    }

    async fn cancel(&self, _id: &RemotePairingCodeId, _now: &str) -> AppResult<bool> {
        Err(AppError::Database("database is locked".to_string()))
    }
}

struct FailingAuditRepository;

#[async_trait]
impl RemoteAuditLogRepository for FailingAuditRepository {
    async fn record(
        &self,
        _device_id: Option<&RemoteDeviceId>,
        _action: RemoteAuditAction,
        _detail: Option<&str>,
        _now: &str,
    ) -> AppResult<()> {
        Err(AppError::Database("database is locked".to_string()))
    }

    async fn list_recent(&self, _limit: Option<i64>) -> AppResult<Vec<RemoteAuditEntry>> {
        Err(AppError::Database("database is locked".to_string()))
    }

    async fn prune_before(&self, _cutoff: &str) -> AppResult<usize> {
        Err(AppError::Database("database is locked".to_string()))
    }
}

// ---------------------------------------------------------------------------------------
// Credential shape
// ---------------------------------------------------------------------------------------

#[test]
fn remote_credentials_use_distinct_greppable_prefixes() {
    let token = generate_device_token();
    let code = generate_pairing_code();
    let ticket = generate_ws_ticket();

    assert!(token.starts_with(REMOTE_DEVICE_TOKEN_PREFIX));
    assert!(code.starts_with(REMOTE_PAIRING_CODE_PREFIX));
    assert!(ticket.starts_with(REMOTE_WS_TICKET_PREFIX));
    assert_eq!(token.len(), REMOTE_DEVICE_TOKEN_PREFIX.len() + 32);
    assert_ne!(token, generate_device_token(), "tokens must not repeat");
    assert!(
        !token.starts_with("rxk_live_"),
        "remote device tokens must be distinguishable from :3848 api keys"
    );
    assert_eq!(device_token_prefix(&token), token[..13].to_string());
}

// ---------------------------------------------------------------------------------------
// P-8: fail-closed middleware
// ---------------------------------------------------------------------------------------

/// Absent, malformed, unknown, and revoked are all 401 — and all say the same thing, so the
/// endpoint is not an oracle for which tokens exist.
#[tokio::test]
async fn every_bad_bearer_shape_is_refused_with_401() {
    let context = in_memory_auth_context();
    let (token, device_id) = pair_device(&context, "laptop").await;
    context
        .devices
        .revoke(&device_id, &now_timestamp())
        .await
        .expect("revoke should succeed");

    let mut observed = Vec::new();
    for request in [
        get_with_bearer(SESSION_PATH, None),
        Request::builder()
            .method(Method::GET)
            .uri(SESSION_PATH)
            .header(header::AUTHORIZATION, "Basic abc123")
            .body(Body::empty())
            .expect("request should build"),
        Request::builder()
            .method(Method::GET)
            .uri(SESSION_PATH)
            .header(header::AUTHORIZATION, "Bearer ")
            .body(Body::empty())
            .expect("request should build"),
        get_with_bearer(SESSION_PATH, Some("rxd_live_notarealtokenatallnotarealto")),
        get_with_bearer(SESSION_PATH, Some(&token)),
    ] {
        let response = router_for(&context)
            .oneshot(request)
            .await
            .expect("request should complete");
        let status = response.status();
        let body = body_json(response).await;
        observed.push((status, body["code"].clone(), body["message"].clone()));
    }

    for (status, code, message) in &observed {
        assert_eq!(*status, StatusCode::UNAUTHORIZED);
        assert_eq!(*code, Value::from("REMOTE_UNAUTHORIZED"));
        assert_eq!(*message, observed[0].2, "messages must not leak which case");
    }
}

/// The heart of the fail-closed contract: a store failure is 500, never a 401 that would let
/// an outage look like "this token is not paired".
#[tokio::test]
async fn a_device_store_failure_answers_500_not_401() {
    let mut context = in_memory_auth_context();
    context.devices = Arc::new(FailingDeviceRepository);

    let response = router_for(&context)
        .oneshot(get_with_bearer(SESSION_PATH, Some("rxd_live_anything")))
        .await
        .expect("request should complete");

    let status = response.status();
    let body = body_json(response).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_ne!(
        status,
        StatusCode::UNAUTHORIZED,
        "a store outage must never be reported as an auth failure"
    );
    assert_eq!(body["code"], Value::from("REMOTE_UNREACHABLE"));
}

/// The three rejection classes are distinct values, not one collapsed variant.
#[test]
fn absent_invalid_and_store_error_are_separate_typed_rejections() {
    let absent = RemoteAuthRejection::MissingBearer;
    let invalid = RemoteAuthRejection::UnknownToken;
    let store = RemoteAuthRejection::StoreUnavailable("locked".to_string());

    assert_ne!(absent, invalid);
    assert_ne!(invalid, store);
    assert_eq!(absent.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(store.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(invalid.counts_as_auth_failure());
    assert!(
        !store.counts_as_auth_failure(),
        "a flaky store must not lock the owner out"
    );
    assert!(!absent.counts_as_auth_failure());
    assert_eq!(
        RemoteAuthRejection::RateLimited {
            retry_after_secs: 7
        }
        .audit_action(),
        RemoteAuditAction::RateLimited
    );
    assert_eq!(
        RemoteAuthRejection::TooManyConcurrentRequests.audit_action(),
        RemoteAuditAction::RateLimited
    );
}

#[test]
fn every_remote_scope_has_the_protocol_label() {
    assert_eq!(scope_label(Scope::UiRead), "ui:read");
    assert_eq!(scope_label(Scope::UiOperate), "ui:operate");
    assert_eq!(scope_label(Scope::UiAgent), "ui:agent");
    assert_eq!(scope_label(Scope::UiElevated), "ui:elevated");
}

#[tokio::test]
async fn audit_store_failures_are_reported_to_authorizing_callers() {
    let mut context = in_memory_auth_context();
    context.audit = Arc::new(FailingAuditRepository);

    assert!(
        !context
            .record_audit(
                None,
                RemoteAuditAction::AuthAccepted,
                Some("GET /remote/v1/session")
            )
            .await
    );
}

/// A-2: a host with zero paired devices answers only the descriptor and `/pair`.
#[tokio::test]
async fn a_host_with_no_paired_devices_still_refuses_every_other_route() {
    let context = in_memory_auth_context();
    let router = router_for(&context);

    let descriptor = router
        .clone()
        .oneshot(get_with_bearer(DESCRIPTOR_PATH, None))
        .await
        .expect("descriptor request should complete");
    let health = router
        .clone()
        .oneshot(get_with_bearer(HEALTH_PATH, None))
        .await
        .expect("health request should complete");
    let session = router
        .oneshot(get_with_bearer(SESSION_PATH, None))
        .await
        .expect("session request should complete");

    assert_eq!(descriptor.status(), StatusCode::OK);
    assert_eq!(health.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(session.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------------------
// Trust-header stripping
// ---------------------------------------------------------------------------------------

async fn echo_header_names(headers: HeaderMap) -> Json<Vec<String>> {
    Json(headers.keys().map(|name| name.to_string()).collect())
}

#[tokio::test]
async fn every_ralphx_trust_header_is_stripped_before_any_handler_runs() {
    let router = Router::new()
        .route("/echo", get(echo_header_names))
        .layer(middleware::from_fn(strip_trust_headers));
    let mut request = Request::builder().method(Method::GET).uri("/echo");
    for name in STRIPPED_TRUST_HEADERS {
        request = request.header(*name, "1");
    }
    let request = request
        .header("x-ralphx-some-future-header", "1")
        .header("x-forwarded-for", "100.64.0.9")
        .body(Body::empty())
        .expect("request should build");

    let response = router
        .oneshot(request)
        .await
        .expect("echo request should complete");

    let seen: Vec<String> =
        serde_json::from_value(body_json(response).await).expect("header names should parse");
    for name in STRIPPED_TRUST_HEADERS {
        assert!(!seen.iter().any(|header| header == name), "{name} leaked");
    }
    assert!(
        !seen
            .iter()
            .any(|header| header.starts_with(RALPHX_HEADER_NAMESPACE)),
        "the whole vendor header namespace must be dropped: {seen:?}"
    );
    assert!(
        seen.iter().any(|header| header == "x-forwarded-for"),
        "unrelated headers must survive"
    );
}

/// Acceptance: `X-RalphX-Tauri-MCP: 1` buys nothing on :3849 — there is no `tauri-local`
/// identity path to reach (feeds P-1).
#[tokio::test]
async fn a_forged_tauri_local_trust_header_still_gets_401() {
    let context = in_memory_auth_context();
    let request = Request::builder()
        .method(Method::GET)
        .uri(SESSION_PATH)
        .header("x-ralphx-tauri-mcp", "1")
        .header("x-ralphx-external-mcp", "1")
        .header("x-ralphx-key-id", "any-key")
        .body(Body::empty())
        .expect("request should build");

    let response = router_for(&context)
        .oneshot(request)
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn pairing_returns_the_token_once_and_grants_read_plus_operate_without_agent_control() {
    let context = in_memory_auth_context();

    let (token, device_id) = pair_device(&context, "laptop").await;

    let device = context
        .devices
        .get(&device_id)
        .await
        .expect("device should read")
        .expect("device should exist");
    assert!(token.starts_with(REMOTE_DEVICE_TOKEN_PREFIX));
    assert_eq!(device.scopes, RemoteScopeSet::default_pairing_grant());
    assert!(!device.agent_control_granted());
    assert_eq!(device.token_hash, hash_key(&token));
    assert_ne!(device.token_hash, token);
    assert!(audit_actions(&context)
        .await
        .contains(&"pairing_succeeded".to_string()));
}

#[tokio::test]
async fn a_replayed_pairing_code_is_refused_and_mints_no_second_device() {
    let context = in_memory_auth_context();
    let code = mint_pairing_code(&context, RemoteScopeSet::default_pairing_grant()).await;
    let request =
        || json!({"pairingCode": code, "deviceName": "laptop", "clientVersion": "0.81.0"});

    let first = router_for(&context)
        .oneshot(post_json(PAIR_PATH, None, request()))
        .await
        .expect("first pair should complete");
    let replay = router_for(&context)
        .oneshot(post_json(PAIR_PATH, None, request()))
        .await
        .expect("replay should complete");

    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        context
            .devices
            .list()
            .await
            .expect("devices should read")
            .len(),
        1
    );
}

#[tokio::test]
async fn a_pairing_request_may_not_ask_for_more_than_the_code_grants() {
    let context = in_memory_auth_context();
    let code = mint_pairing_code(&context, RemoteScopeSet::from_scopes([Scope::UiRead])).await;

    let response = router_for(&context)
        .oneshot(post_json(
            PAIR_PATH,
            None,
            json!({
                "pairingCode": code,
                "deviceName": "laptop",
                "requestedScopes": ["ui:read", "ui:agent"],
            }),
        ))
        .await
        .expect("pair request should complete");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(context
        .devices
        .list()
        .await
        .expect("devices should read")
        .is_empty());
}

#[tokio::test]
async fn a_pairing_request_may_narrow_its_own_grant() {
    let context = in_memory_auth_context();
    let code = mint_pairing_code(&context, RemoteScopeSet::default_pairing_grant()).await;

    let response = router_for(&context)
        .oneshot(post_json(
            PAIR_PATH,
            None,
            json!({
                "pairingCode": code,
                "deviceName": "read-only tablet",
                "requestedScopes": ["ui:read"],
            }),
        ))
        .await
        .expect("pair request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["scopes"], json!(["ui:read"]));
}

#[tokio::test]
async fn pairing_rejects_blank_and_oversized_device_names() {
    let context = in_memory_auth_context();

    for device_name in ["   ".to_string(), "x".repeat(121)] {
        let code = mint_pairing_code(&context, RemoteScopeSet::default_pairing_grant()).await;
        let response = router_for(&context)
            .oneshot(post_json(
                PAIR_PATH,
                None,
                json!({"pairingCode": code, "deviceName": device_name}),
            ))
            .await
            .expect("pair request should complete");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = body_json(response).await;
        assert_eq!(body["code"], Value::from("REMOTE_FORBIDDEN"));
    }

    assert!(context
        .devices
        .list()
        .await
        .expect("devices should read")
        .is_empty());
    assert!(audit_actions(&context)
        .await
        .contains(&"pairing_rejected".to_string()));
}

/// Acceptance: the 6th failed pair attempt inside the window is rate limited.
#[tokio::test]
async fn the_sixth_failed_pair_attempt_is_rate_limited() {
    let context = in_memory_auth_context();
    let bad_code = "rxp_thiscodewasnevermintedbyanyhost";

    let mut statuses = Vec::new();
    for _ in 0..6 {
        let response = router_for(&context)
            .oneshot(post_json(
                PAIR_PATH,
                None,
                json!({"pairingCode": bad_code, "deviceName": "attacker"}),
            ))
            .await
            .expect("pair attempt should complete");
        statuses.push(response.status());
    }

    assert_eq!(
        statuses[..5],
        [StatusCode::UNAUTHORIZED; 5],
        "the first five attempts are ordinary refusals: {statuses:?}"
    );
    assert_eq!(statuses[5], StatusCode::TOO_MANY_REQUESTS);
}

/// P-8: under Serve one peer's lockout must not reach another device's pairing attempt.
#[tokio::test]
async fn a_locked_out_pairing_code_does_not_lock_out_a_different_code() {
    let context = in_memory_auth_context();
    let attacked = "rxp_thiscodewasnevermintedbyanyhost";
    for _ in 0..6 {
        router_for(&context)
            .oneshot(post_json(
                PAIR_PATH,
                None,
                json!({"pairingCode": attacked, "deviceName": "attacker"}),
            ))
            .await
            .expect("pair attempt should complete");
    }
    let owner_code = mint_pairing_code(&context, RemoteScopeSet::default_pairing_grant()).await;

    let response = router_for(&context)
        .oneshot(post_json(
            PAIR_PATH,
            None,
            json!({"pairingCode": owner_code, "deviceName": "owner phone"}),
        ))
        .await
        .expect("owner pair should complete");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the owner's own device must still be able to pair"
    );
}

/// A pairing attempt that fails because the *store* failed must not read as a bad code, and
/// must leave the code redeemable.
#[tokio::test]
async fn a_pairing_store_failure_is_500_and_leaves_the_code_redeemable() {
    let context = in_memory_auth_context();
    let code = mint_pairing_code(&context, RemoteScopeSet::default_pairing_grant()).await;
    let mut broken = context.clone();
    broken.pairing_codes = Arc::new(FailingPairingCodeRepository);

    let failed = router_for(&broken)
        .oneshot(post_json(
            PAIR_PATH,
            None,
            json!({"pairingCode": code, "deviceName": "laptop"}),
        ))
        .await
        .expect("pair request should complete");
    let recovered = router_for(&context)
        .oneshot(post_json(
            PAIR_PATH,
            None,
            json!({"pairingCode": code, "deviceName": "laptop"}),
        ))
        .await
        .expect("pair request should complete");

    assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(recovered.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------------------
// WS tickets
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn ws_tickets_are_issued_to_authenticated_devices_and_consumed_once() {
    let context = in_memory_auth_context();
    let (token, device_id) = pair_device(&context, "laptop").await;

    let response = router_for(&context)
        .oneshot(post_json(WS_TICKET_PATH, Some(&token), json!({})))
        .await
        .expect("ws-ticket request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let ticket = body["ticket"]
        .as_str()
        .expect("a ticket is returned")
        .to_string();
    assert!(ticket.starts_with(REMOTE_WS_TICKET_PREFIX));
    assert_eq!(body["expiresInSecs"], json!(WS_TICKET_TTL_SECS));

    let first = context
        .tickets
        .consume(&hash_key(&ticket), &now_timestamp())
        .await
        .expect("consume should complete");
    let replay = context
        .tickets
        .consume(&hash_key(&ticket), &now_timestamp())
        .await
        .expect("replay should complete");
    assert_eq!(first, RemoteWsTicketOutcome::Consumed(device_id));
    assert_eq!(replay, RemoteWsTicketOutcome::AlreadyConsumed);
}

#[tokio::test]
async fn an_expired_or_unknown_ws_ticket_is_never_consumable() {
    let context = in_memory_auth_context();
    let (_, device_id) = pair_device(&context, "laptop").await;
    let stale = generate_ws_ticket();
    context
        .tickets
        .issue(
            &hash_key(&stale),
            &device_id,
            &expiry_timestamp(chrono::Utc::now() - chrono::Duration::seconds(120), 60),
        )
        .await
        .expect("ticket should issue");

    let expired = context
        .tickets
        .consume(&hash_key(&stale), &now_timestamp())
        .await
        .expect("consume should complete");
    let unknown = context
        .tickets
        .consume(&hash_key("rxt_neverissued"), &now_timestamp())
        .await
        .expect("consume should complete");

    assert_eq!(expired, RemoteWsTicketOutcome::Expired);
    assert_eq!(unknown, RemoteWsTicketOutcome::Unknown);
}

#[tokio::test]
async fn an_unauthenticated_caller_cannot_mint_a_ws_ticket() {
    let context = in_memory_auth_context();

    let response = router_for(&context)
        .oneshot(post_json(WS_TICKET_PATH, None, json!({})))
        .await
        .expect("ws-ticket request should complete");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------------------
// P-28 host half: agent control
// ---------------------------------------------------------------------------------------

async fn session_introspection(context: &RemoteAuthContext, token: &str) -> Value {
    let response = router_for(context)
        .oneshot(get_with_bearer(SESSION_PATH, Some(token)))
        .await
        .expect("session request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await
}

#[tokio::test]
async fn agent_control_is_off_by_default_and_toggles_without_re_pairing() {
    let context = in_memory_auth_context();
    let (token, device_id) = pair_device(&context, "laptop").await;

    let paired = session_introspection(&context, &token).await;
    let device = context
        .devices
        .get(&device_id)
        .await
        .expect("device should read")
        .expect("device should exist");
    context
        .devices
        .set_scopes(&device_id, &device.scopes.with(Scope::UiAgent))
        .await
        .expect("grant should apply");
    let granted = session_introspection(&context, &token).await;
    context
        .devices
        .set_scopes(&device_id, &device.scopes.without(Scope::UiAgent))
        .await
        .expect("narrow should apply");
    let narrowed = session_introspection(&context, &token).await;

    assert_eq!(paired["agentControlGranted"], json!(false));
    assert_eq!(paired["scopes"], json!(["ui:read", "ui:operate"]));
    assert_eq!(granted["agentControlGranted"], json!(true));
    assert_eq!(
        granted["scopes"],
        json!(["ui:read", "ui:operate", "ui:agent"])
    );
    assert_eq!(narrowed["agentControlGranted"], json!(false));
    assert_eq!(narrowed["scopes"], json!(["ui:read", "ui:operate"]));
}

/// Narrowing the grant must also fire the device's kill channels — introspection alone is
/// not teardown.
#[tokio::test]
async fn withdrawing_agent_control_tears_the_devices_live_sessions_down() {
    let context = in_memory_auth_context();
    let (_, device_id) = pair_device(&context, "laptop").await;
    let session_id = RemoteSessionId::new();
    let RemoteSessionAdmission::Admitted(mut kill) =
        context.registry.register(&device_id, &session_id)
    else {
        panic!("the first session should be admitted");
    };
    context
        .sessions
        .open(RemoteSession {
            id: session_id.clone(),
            device_id: device_id.clone(),
            connected_at: now_timestamp(),
            last_active_at: now_timestamp(),
            remote_addr: "127.0.0.1:51000".to_string(),
            closed_at: None,
        })
        .await
        .expect("session row should open");

    let device = context
        .devices
        .get(&device_id)
        .await
        .expect("device should read")
        .expect("device should exist");
    context
        .devices
        .set_scopes(&device_id, &device.scopes.without(Scope::UiAgent))
        .await
        .expect("narrow should apply");
    let torn_down = context
        .tear_down_device_sessions(&device_id, ResetReason::Revoked)
        .await;

    assert_eq!(torn_down, 1);
    assert_eq!(kill.try_recv(), Some(ResetReason::Revoked));
    assert_eq!(context.registry.device_session_count(&device_id), 0);
    assert!(context
        .sessions
        .list_open()
        .await
        .expect("sessions should read")
        .is_empty());
}

#[tokio::test]
async fn revoking_a_device_refuses_its_next_request_and_kills_its_sessions() {
    let context = in_memory_auth_context();
    let (token, device_id) = pair_device(&context, "laptop").await;
    let RemoteSessionAdmission::Admitted(mut kill) = context
        .registry
        .register(&device_id, &RemoteSessionId::new())
    else {
        panic!("the first session should be admitted");
    };

    context
        .devices
        .revoke(&device_id, &now_timestamp())
        .await
        .expect("revoke should succeed");
    let torn_down = context
        .tear_down_device_sessions(&device_id, ResetReason::Revoked)
        .await;
    let after = router_for(&context)
        .oneshot(get_with_bearer(SESSION_PATH, Some(&token)))
        .await
        .expect("request should complete");

    assert_eq!(torn_down, 1);
    assert_eq!(kill.try_recv(), Some(ResetReason::Revoked));
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED);
}

/// Revocation must also burn the device's outstanding upgrade tickets, or a ticket minted
/// seconds earlier would still buy a socket.
#[tokio::test]
async fn revocation_invalidates_outstanding_ws_tickets() {
    let context = in_memory_auth_context();
    let (token, device_id) = pair_device(&context, "laptop").await;
    let ticket_response = router_for(&context)
        .oneshot(post_json(WS_TICKET_PATH, Some(&token), json!({})))
        .await
        .expect("ws-ticket request should complete");
    let ticket = body_json(ticket_response).await["ticket"]
        .as_str()
        .expect("a ticket is returned")
        .to_string();

    context
        .devices
        .revoke(&device_id, &now_timestamp())
        .await
        .expect("revoke should succeed");
    context
        .tear_down_device_sessions(&device_id, ResetReason::Revoked)
        .await;

    assert_eq!(
        context
            .tickets
            .consume(&hash_key(&ticket), &now_timestamp())
            .await
            .expect("consume should complete"),
        RemoteWsTicketOutcome::AlreadyConsumed
    );
}

#[tokio::test]
async fn deleting_a_named_owned_session_closes_only_that_session() {
    let context = in_memory_auth_context();
    let (token, device_id) = pair_device(&context, "laptop").await;
    let named_session_id = RemoteSessionId::new();
    let surviving_session_id = RemoteSessionId::new();
    let RemoteSessionAdmission::Admitted(mut named_kill) =
        context.registry.register(&device_id, &named_session_id)
    else {
        panic!("the named session should be admitted");
    };
    let RemoteSessionAdmission::Admitted(mut surviving_kill) =
        context.registry.register(&device_id, &surviving_session_id)
    else {
        panic!("the second session should be admitted");
    };

    let response = router_for(&context)
        .oneshot(delete_with_bearer(
            &format!("{SESSION_PATH}?sessionId={named_session_id}"),
            &token,
        ))
        .await
        .expect("delete should complete");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["closedSessions"], json!(1));
    assert_eq!(named_kill.try_recv(), Some(ResetReason::Revoked));
    assert_eq!(
        surviving_kill.try_recv(),
        None,
        "another session on the same device must stay live"
    );
    assert_eq!(
        context.registry.live_sessions(&device_id),
        vec![surviving_session_id]
    );
}

#[tokio::test]
async fn deleting_a_session_the_device_does_not_own_returns_404_and_closes_nothing() {
    let context = in_memory_auth_context();
    let (token, device_id) = pair_device(&context, "laptop").await;
    let (_, other_device_id) = pair_device(&context, "phone").await;
    let own_session_id = RemoteSessionId::new();
    let other_session_id = RemoteSessionId::new();
    let RemoteSessionAdmission::Admitted(mut own_kill) =
        context.registry.register(&device_id, &own_session_id)
    else {
        panic!("the caller session should be admitted");
    };
    let RemoteSessionAdmission::Admitted(mut other_kill) = context
        .registry
        .register(&other_device_id, &other_session_id)
    else {
        panic!("the other device session should be admitted");
    };

    let response = router_for(&context)
        .oneshot(delete_with_bearer(
            &format!("{SESSION_PATH}?sessionId={other_session_id}"),
            &token,
        ))
        .await
        .expect("delete should complete");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(own_kill.try_recv(), None);
    assert_eq!(other_kill.try_recv(), None);
    assert_eq!(
        context.registry.live_sessions(&device_id),
        vec![own_session_id]
    );
    assert_eq!(
        context.registry.live_sessions(&other_device_id),
        vec![other_session_id]
    );
}

#[tokio::test]
async fn deleting_without_a_session_id_closes_every_session_on_the_callers_device_only() {
    let context = in_memory_auth_context();
    let (token, device_id) = pair_device(&context, "laptop").await;
    let (_, bystander_id) = pair_device(&context, "phone").await;
    let RemoteSessionAdmission::Admitted(mut first_kill) = context
        .registry
        .register(&device_id, &RemoteSessionId::new())
    else {
        panic!("the first session should be admitted");
    };
    let RemoteSessionAdmission::Admitted(mut second_kill) = context
        .registry
        .register(&device_id, &RemoteSessionId::new())
    else {
        panic!("the second session should be admitted");
    };
    let RemoteSessionAdmission::Admitted(mut bystander_kill) = context
        .registry
        .register(&bystander_id, &RemoteSessionId::new())
    else {
        panic!("the bystander session should be admitted");
    };

    let response = router_for(&context)
        .oneshot(delete_with_bearer(SESSION_PATH, &token))
        .await
        .expect("delete should complete");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["closedSessions"], json!(2));
    assert_eq!(first_kill.try_recv(), Some(ResetReason::Revoked));
    assert_eq!(second_kill.try_recv(), Some(ResetReason::Revoked));
    assert_eq!(
        bystander_kill.try_recv(),
        None,
        "another device's session must survive"
    );
}

/// P-8, the path the per-code independence test does not reach: bearer failures arrive on the
/// AUTHENTICATED routes, and under Serve every caller looks like loopback. If those failures
/// were charged to the shared pre-auth key, five garbage bearers on `/remote/v1/session` would
/// lock every device out of pairing and ws-ticket minting for 30 s.
#[tokio::test]
async fn bad_bearers_on_an_authenticated_route_never_lock_out_pairing() {
    let context = in_memory_auth_context();

    for attempt in 0..8 {
        let response = router_for(&context)
            .oneshot(get_with_bearer(
                SESSION_PATH,
                Some(&format!("rxd_live_garbageguess{attempt:015}")),
            ))
            .await
            .expect("bad-bearer request should complete");
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "attempt {attempt} should be an ordinary refusal, not a lockout"
        );
    }

    let code = mint_pairing_code(&context, RemoteScopeSet::default_pairing_grant()).await;
    let paired = router_for(&context)
        .oneshot(post_json(
            PAIR_PATH,
            None,
            json!({"pairingCode": code, "deviceName": "owner phone"}),
        ))
        .await
        .expect("pair request should complete");

    assert_eq!(
        paired.status(),
        StatusCode::OK,
        "a hostile peer's bearer guesses must not deny pairing to other devices"
    );
}

/// The same isolation in the other direction: repeated failures on ONE token must not deny a
/// different, valid device.
#[tokio::test]
async fn a_locked_out_bearer_does_not_lock_out_a_paired_device() {
    let context = in_memory_auth_context();
    let (token, _) = pair_device(&context, "laptop").await;
    for _ in 0..6 {
        router_for(&context)
            .oneshot(get_with_bearer(
                SESSION_PATH,
                Some("rxd_live_thesamewrongguessrepeated"),
            ))
            .await
            .expect("bad-bearer request should complete");
    }

    let response = router_for(&context)
        .oneshot(get_with_bearer(SESSION_PATH, Some(&token)))
        .await
        .expect("authenticated request should complete");

    assert_eq!(response.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------------------
// Self-revocation (§6.1 removal ordering / P-27)
// ---------------------------------------------------------------------------------------

/// The client's staged remove/re-pair machines call this route; without it their "host revoke"
/// step hits the router fallback and the device row stays live forever.
#[tokio::test]
async fn a_device_can_revoke_its_own_token_and_is_refused_afterwards() {
    let context = in_memory_auth_context();
    let (token, device_id) = pair_device(&context, "laptop").await;
    let (bystander_token, _) = pair_device(&context, "phone").await;

    let revoked = router_for(&context)
        .oneshot(post_json(REVOKE_PATH, Some(&token), json!({})))
        .await
        .expect("self-revoke should complete");
    assert_eq!(revoked.status(), StatusCode::OK);
    assert_eq!(
        body_json(revoked).await["deviceId"],
        json!(device_id.to_string())
    );

    let replayed = router_for(&context)
        .oneshot(get_with_bearer(SESSION_PATH, Some(&token)))
        .await
        .expect("post-revoke request should complete");
    let bystander = router_for(&context)
        .oneshot(get_with_bearer(SESSION_PATH, Some(&bystander_token)))
        .await
        .expect("bystander request should complete");
    let stored = context
        .devices
        .get(&device_id)
        .await
        .expect("device should read")
        .expect("device row should survive revocation");

    assert_eq!(replayed.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        bystander.status(),
        StatusCode::OK,
        "self-revocation is self-scoped: no device may revoke another"
    );
    assert!(stored.revoked_at.is_some());
    assert!(audit_actions(&context)
        .await
        .contains(&"device_revoked".to_string()));
}

/// Self-revocation must reach the live-session registry, not just the row (§4.4).
#[tokio::test]
async fn self_revocation_tears_down_the_devices_live_sessions() {
    let context = in_memory_auth_context();
    let (token, device_id) = pair_device(&context, "laptop").await;
    let session_id = RemoteSessionId::new();
    let RemoteSessionAdmission::Admitted(mut kill) =
        context.registry.register(&device_id, &session_id)
    else {
        panic!("a fresh session should be admitted");
    };

    router_for(&context)
        .oneshot(post_json(REVOKE_PATH, Some(&token), json!({})))
        .await
        .expect("self-revoke should complete");

    assert_eq!(kill.try_recv(), Some(ResetReason::Revoked));
}

/// C-11 parity: the two hand-written pairing DTOs (host `PairRequest`/`PairResponse` and
/// client `PairWire*`) must survive a real round trip through each other, so a rename on one
/// side fails here instead of at runtime.
#[test]
fn the_pair_wire_shapes_round_trip_between_host_and_client() {
    use crate::infrastructure::remote_host_client::{PairWireRequest, PairWireResponse};
    use crate::remote_server::auth_endpoints::{PairRequest, PairResponse};

    let client_request = PairWireRequest {
        pairing_code: "rxp_0123456789abcdefghijklmnopqrstuv".to_string(),
        device_name: "RalphX Desktop 0.81.0".to_string(),
        client_version: "0.81.0".to_string(),
        requested_scopes: vec![Scope::UiRead, Scope::UiOperate],
    };
    let host_request: PairRequest =
        serde_json::from_value(serde_json::to_value(&client_request).expect("client serializes"))
            .expect("the host must parse what the client sends");

    let host_response = PairResponse {
        device_token: "rxd_live_secret".to_string(),
        device_id: "device-1".to_string(),
        scopes: vec![Scope::UiRead],
        environment_id: TEST_ENVIRONMENT_ID.to_string(),
        protocol_version: ralphx_remote_protocol::PROTOCOL_VERSION,
    };
    let client_response: PairWireResponse =
        serde_json::from_value(serde_json::to_value(&host_response).expect("host serializes"))
            .expect("the client must parse what the host answers");

    assert_eq!(host_request.pairing_code, client_request.pairing_code);
    assert_eq!(host_request.device_name, client_request.device_name);
    assert_eq!(
        host_request.client_version.as_deref(),
        Some(client_request.client_version.as_str()),
        "the host audits clientVersion; the client must actually send it"
    );
    assert_eq!(
        host_request.requested_scopes,
        Some(client_request.requested_scopes)
    );
    assert_eq!(client_response.device_token, host_response.device_token);
    assert_eq!(client_response.environment_id, host_response.environment_id);
    assert_eq!(
        client_response.protocol_version,
        Some(host_response.protocol_version)
    );
}

// ---------------------------------------------------------------------------------------
// Audit trail
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn every_auth_decision_leaves_an_audit_row() {
    let context = in_memory_auth_context();
    let (token, _) = pair_device(&context, "laptop").await;

    router_for(&context)
        .oneshot(get_with_bearer(SESSION_PATH, Some(&token)))
        .await
        .expect("authenticated request should complete");
    router_for(&context)
        .oneshot(get_with_bearer(SESSION_PATH, Some("rxd_live_wrongtoken")))
        .await
        .expect("rejected request should complete");
    router_for(&context)
        .oneshot(post_json(
            PAIR_PATH,
            None,
            json!({"pairingCode": "rxp_nope", "deviceName": "attacker"}),
        ))
        .await
        .expect("rejected pairing should complete");

    let actions = audit_actions(&context).await;
    for expected in [
        "pairing_succeeded",
        "auth_accepted",
        "auth_rejected",
        "pairing_rejected",
    ] {
        assert!(
            actions.contains(&expected.to_string()),
            "{expected} should be audited: {actions:?}"
        );
    }
}

#[tokio::test]
async fn an_authenticated_request_updates_last_seen_at() {
    let context = in_memory_auth_context();
    let (token, device_id) = pair_device(&context, "laptop").await;
    let before = context
        .devices
        .get(&device_id)
        .await
        .expect("device should read")
        .expect("device should exist");

    router_for(&context)
        .oneshot(get_with_bearer(SESSION_PATH, Some(&token)))
        .await
        .expect("authenticated request should complete");

    let after = context
        .devices
        .get(&device_id)
        .await
        .expect("device should read")
        .expect("device should exist");
    assert!(before.last_seen_at.is_none());
    assert!(after.last_seen_at.is_some());
}
