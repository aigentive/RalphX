use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use ralphx_remote_protocol::PROTOCOL_VERSION;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

use super::endpoints::{environment_descriptor, RemoteRouterState, MIN_CLIENT_PROTOCOL};
use super::settings::{
    RemoteExposureMode, RemoteHostSettingsStore, TailnetProviderError, UnconfiguredTailnetProvider,
    REMOTE_PORT_ENV,
};
use super::{
    allowed_app_origins, apply_exposure_mode as apply_exposure_mode_with_app,
    authenticated_remote_routes, auto_start_if_enabled as auto_start_if_enabled_with_app,
    remote_router, start_listener as start_listener_with_app, stop_listener, RemoteListenerError,
    RemoteListenerHandle, DESCRIPTOR_PATH, HEALTH_PATH, PAIR_PATH, PRE_AUTH_ALLOWLIST,
};
use crate::infrastructure::sqlite::DbConnection;
use crate::infrastructure::tailscale::{TailscaleCommandRunner, TailscaleServeError};
use crate::testing::SqliteTestDb;
use crate::utils::backend_endpoint::{
    backend_http_base_url, backend_http_bind_addr, backend_http_port, PRODUCTION_BACKEND_PORT,
};

const TEST_APP_ORIGIN: &str = "tauri://localhost";

fn test_app_handle() -> tauri::AppHandle {
    tauri::Builder::default()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("test Wry app should build")
        .handle()
        .clone()
}

async fn start_listener(
    handle: &RemoteListenerHandle,
    store: &RemoteHostSettingsStore,
    provider: &dyn super::settings::TailnetSelfAddressProvider,
    tailscale: &dyn TailscaleCommandRunner,
) -> Result<SocketAddr, RemoteListenerError> {
    start_listener_with_app(&test_app_handle(), handle, store, provider, tailscale).await
}

async fn auto_start_if_enabled(
    handle: &RemoteListenerHandle,
    store: &RemoteHostSettingsStore,
    provider: &dyn super::settings::TailnetSelfAddressProvider,
    tailscale: &dyn TailscaleCommandRunner,
) -> Result<Option<SocketAddr>, RemoteListenerError> {
    auto_start_if_enabled_with_app(&test_app_handle(), handle, store, provider, tailscale).await
}

async fn apply_exposure_mode(
    handle: &RemoteListenerHandle,
    store: &RemoteHostSettingsStore,
    provider: &dyn super::settings::TailnetSelfAddressProvider,
    tailscale: &dyn TailscaleCommandRunner,
    exposure_mode: RemoteExposureMode,
) -> Result<super::settings::RemoteHostSettings, RemoteListenerError> {
    apply_exposure_mode_with_app(
        &test_app_handle(),
        handle,
        store,
        provider,
        tailscale,
        exposure_mode,
    )
    .await
}

struct ConfiguredTailnetProvider;

#[async_trait]
impl super::settings::TailnetSelfAddressProvider for ConfiguredTailnetProvider {
    async fn self_addresses(&self) -> Result<Vec<IpAddr>, TailnetProviderError> {
        Ok(vec![IpAddr::V4(Ipv4Addr::new(100, 101, 102, 103))])
    }
}

#[derive(Clone, Default)]
struct RecordingTailscaleCommandRunner {
    calls: Arc<Mutex<Vec<TailscaleCall>>>,
    acquire_error: Option<TailscaleServeError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TailscaleCall {
    Acquire(u16),
    Release,
}

impl RecordingTailscaleCommandRunner {
    fn failing_acquire(error: TailscaleServeError) -> Self {
        Self {
            calls: Arc::default(),
            acquire_error: Some(error),
        }
    }

    fn calls(&self) -> Vec<TailscaleCall> {
        self.calls.lock().expect("command recorder mutex").clone()
    }
}

#[async_trait]
impl TailscaleCommandRunner for RecordingTailscaleCommandRunner {
    async fn run_status(&self) -> Result<String, TailnetProviderError> {
        Ok(String::new())
    }

    async fn run_serve_acquire(&self, port: u16) -> Result<(), TailscaleServeError> {
        self.calls
            .lock()
            .expect("command recorder mutex")
            .push(TailscaleCall::Acquire(port));
        match self.acquire_error.as_ref() {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    async fn run_serve_release(&self) -> Result<(), TailscaleServeError> {
        self.calls
            .lock()
            .expect("command recorder mutex")
            .push(TailscaleCall::Release);
        Ok(())
    }
}

async fn response_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    serde_json::from_slice(&bytes).expect("response body should be JSON")
}

fn descriptor_state() -> RemoteRouterState {
    RemoteRouterState::new(
        "11111111-2222-3333-4444-555555555555",
        super::auth_tests::in_memory_auth_context(),
        test_app_handle(),
    )
}

fn preflight_request(path: &str, origin: &str) -> Request<Body> {
    Request::builder()
        .method(Method::OPTIONS)
        .uri(path)
        .header(header::ORIGIN, origin)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, Method::POST.as_str())
        .header(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            "authorization,content-type",
        )
        .body(Body::empty())
        .expect("preflight request should be valid")
}

fn get_request(path: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .expect("request should be valid")
}

/// Reserves a loopback port and releases it so the listener can claim it.
async fn reserve_loopback_port() -> u16 {
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback probe socket should bind");
    probe
        .local_addr()
        .expect("probe socket should report its address")
        .port()
}

fn set_configured_port(db: &SqliteTestDb, port: u16) {
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE remote_host_settings SET port = ?1 WHERE id = 1",
            rusqlite::params![i64::from(port)],
        )
        .expect("configured port should update");
    });
}

async fn http_get_over_socket(address: SocketAddr, path: &str) -> std::io::Result<(u16, String)> {
    let mut stream = tokio::net::TcpStream::connect(address).await?;
    let request = format!("GET {path} HTTP/1.0\r\nHost: {address}\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await?;
    let response = String::from_utf8_lossy(&buffer).to_string();
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|status| status.parse::<u16>().ok())
        .unwrap_or_default();
    Ok((status, response))
}

#[tokio::test]
async fn descriptor_returns_exactly_the_five_camel_case_fields() {
    let router = remote_router(descriptor_state());

    let response = router
        .oneshot(get_request(DESCRIPTOR_PATH))
        .await
        .expect("descriptor request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_body(response).await;
    let object = body.as_object().expect("descriptor should be an object");
    let fields = object.keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        fields,
        BTreeSet::from([
            "appVersion".to_string(),
            "environmentId".to_string(),
            "minClientProtocol".to_string(),
            "platform".to_string(),
            "protocolVersion".to_string(),
        ])
    );
    assert_eq!(
        object["environmentId"],
        Value::from("11111111-2222-3333-4444-555555555555")
    );
    assert_eq!(object["appVersion"], Value::from(env!("CARGO_PKG_VERSION")));
    assert_eq!(object["protocolVersion"], Value::from(PROTOCOL_VERSION));
    assert_eq!(
        object["minClientProtocol"],
        Value::from(MIN_CLIENT_PROTOCOL)
    );
    assert_eq!(object["platform"], Value::from(std::env::consts::OS));
}

#[tokio::test]
async fn descriptor_environment_id_survives_a_settings_store_restart() {
    let db = SqliteTestDb::new("remote-listener-descriptor-identity");
    let first_store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    let first = first_store
        .get_or_create()
        .await
        .expect("first access should mint settings");
    let reopened_store = RemoteHostSettingsStore::from_db(DbConnection::new(db.new_connection()));
    let reopened = reopened_store
        .get_or_create()
        .await
        .expect("reopened access should read settings");

    let first_descriptor = environment_descriptor(&first.environment_id);
    let reopened_descriptor = environment_descriptor(&reopened.environment_id);

    assert_eq!(
        first_descriptor.environment_id,
        reopened_descriptor.environment_id
    );
    assert_eq!(first_descriptor, reopened_descriptor);
}

#[tokio::test]
async fn every_non_allowlisted_route_fails_closed_without_a_bearer() {
    let router = remote_router(descriptor_state());

    let health = router
        .clone()
        .oneshot(get_request(HEALTH_PATH))
        .await
        .expect("health request should complete");
    let invoke = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/remote/v1/invoke")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("invoke request should complete");

    assert_eq!(health.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(invoke.status(), StatusCode::UNAUTHORIZED);
    let body = response_body(health).await;
    assert_eq!(body["code"], Value::from("REMOTE_UNAUTHORIZED"));
}

#[test]
fn the_pre_auth_allowlist_holds_exactly_the_descriptor_and_pairing_routes() {
    assert_eq!(PRE_AUTH_ALLOWLIST, &[DESCRIPTOR_PATH, PAIR_PATH]);
    assert_eq!(DESCRIPTOR_PATH, "/.well-known/ralphx/environment");
    assert_eq!(PAIR_PATH, "/remote/v1/auth/pair");
}

#[tokio::test]
async fn preflight_succeeds_without_a_bearer_on_any_remote_route() {
    let router = remote_router(descriptor_state());

    for path in [DESCRIPTOR_PATH, HEALTH_PATH, "/remote/v1/invoke"] {
        let response = router
            .clone()
            .oneshot(preflight_request(path, TEST_APP_ORIGIN))
            .await
            .expect("preflight should complete");

        assert!(
            response.status().is_success(),
            "preflight for {path} returned {}",
            response.status()
        );
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|value| value.to_str().ok()),
            Some(TEST_APP_ORIGIN)
        );
    }
}

#[tokio::test]
async fn the_auth_slot_itself_lets_options_through_before_the_bearer_check() {
    // Proves the allowlist ordering rather than relying on the CORS layer short-circuiting.
    let routes = authenticated_remote_routes(descriptor_state());

    for path in [DESCRIPTOR_PATH, HEALTH_PATH, "/remote/v1/invoke"] {
        let response = routes
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri(path)
                    .body(Body::empty())
                    .expect("request should be valid"),
            )
            .await
            .expect("options request should complete");

        assert!(
            response.status().is_success(),
            "unlayered OPTIONS for {path} returned {}",
            response.status()
        );
    }
}

#[tokio::test]
async fn cors_refuses_origins_outside_the_app_origin_list() {
    let router = remote_router(descriptor_state());

    let response = router
        .oneshot(preflight_request(DESCRIPTOR_PATH, "https://evil.example"))
        .await
        .expect("preflight should complete");

    assert!(response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
}

#[test]
fn the_app_origin_list_never_admits_arbitrary_origins() {
    let origins = allowed_app_origins();

    assert!(origins.contains(&"tauri://localhost"));
    assert!(!origins.contains(&"*"));
    assert!(origins.iter().all(|origin| origin.starts_with("tauri://")
        || origin.starts_with("http://127.0.0.1:")
        || origin.starts_with("http://localhost:")));
}

#[tokio::test]
async fn enable_disable_enable_releases_and_reacquires_the_port() {
    // Depends on `RALPHX_REMOTE_PORT` being unset, as the focused Rust gate specifies.
    assert!(
        std::env::var(REMOTE_PORT_ENV).is_err(),
        "{REMOTE_PORT_ENV} must be unset for the lifecycle gate"
    );
    let db = SqliteTestDb::new("remote-listener-lifecycle");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    store
        .get_or_create()
        .await
        .expect("settings should mint before the port is pinned");
    let port = reserve_loopback_port().await;
    set_configured_port(&db, port);
    let handle = RemoteListenerHandle::new();
    let runner = RecordingTailscaleCommandRunner::default();

    let first = start_listener(&handle, &store, &UnconfiguredTailnetProvider, &runner)
        .await
        .expect("serve mode should start");
    let (first_status, _) = http_get_over_socket(first, DESCRIPTOR_PATH)
        .await
        .expect("descriptor should answer on the bound port");
    let enabled_after_start = store
        .get()
        .await
        .expect("settings should read")
        .expect("settings row should exist");
    let stopped = stop_listener(&handle, &store, &runner)
        .await
        .expect("stop should succeed");
    let disabled_after_stop = store
        .get()
        .await
        .expect("settings should read")
        .expect("settings row should exist");
    let closed = http_get_over_socket(first, DESCRIPTOR_PATH).await;
    let second = start_listener(&handle, &store, &UnconfiguredTailnetProvider, &runner)
        .await
        .expect("serve mode should start again on the released port");
    let (second_status, _) = http_get_over_socket(second, DESCRIPTOR_PATH)
        .await
        .expect("descriptor should answer after the restart");
    stop_listener(&handle, &store, &runner)
        .await
        .expect("final stop should succeed");

    assert_eq!(first, SocketAddr::from(([127, 0, 0, 1], port)));
    assert_eq!(first_status, 200);
    assert!(enabled_after_start.enabled);
    assert!(stopped);
    assert!(!disabled_after_stop.enabled);
    assert!(closed.is_err(), "port should be released after a stop");
    assert_eq!(second, first);
    assert_eq!(second_status, 200);
    assert!(!handle.is_running().await);
    assert_eq!(
        runner.calls(),
        vec![
            TailscaleCall::Acquire(port),
            TailscaleCall::Release,
            TailscaleCall::Acquire(port),
            TailscaleCall::Release,
        ]
    );
}

#[tokio::test]
async fn starting_an_already_running_listener_is_idempotent() {
    let db = SqliteTestDb::new("remote-listener-idempotent-start");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    store.get_or_create().await.expect("settings should mint");
    set_configured_port(&db, reserve_loopback_port().await);
    let handle = RemoteListenerHandle::new();
    let runner = RecordingTailscaleCommandRunner::default();

    let first = start_listener(&handle, &store, &UnconfiguredTailnetProvider, &runner)
        .await
        .expect("first start should succeed");
    let second = start_listener(&handle, &store, &UnconfiguredTailnetProvider, &runner)
        .await
        .expect("second start should reuse the running listener");
    stop_listener(&handle, &store, &runner)
        .await
        .expect("stop should succeed");

    assert_eq!(first, second);
    assert_eq!(
        runner.calls(),
        vec![TailscaleCall::Acquire(first.port()), TailscaleCall::Release]
    );
}

#[tokio::test]
async fn serve_start_acquires_bound_port_and_reports_healthy_status() {
    let db = SqliteTestDb::new("remote-listener-serve-healthy");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    store.get_or_create().await.expect("settings should mint");
    let port = reserve_loopback_port().await;
    set_configured_port(&db, port);
    let handle = RemoteListenerHandle::new();
    let runner = RecordingTailscaleCommandRunner::default();

    let address = start_listener(&handle, &store, &UnconfiguredTailnetProvider, &runner)
        .await
        .expect("serve mode should start");
    let status = handle.serve_status().await;
    stop_listener(&handle, &store, &runner)
        .await
        .expect("stop should succeed");

    assert_eq!(address, SocketAddr::from(([127, 0, 0, 1], port)));
    assert!(status.active);
    assert!(status.degraded_reason.is_none());
    assert_eq!(
        runner.calls(),
        vec![TailscaleCall::Acquire(port), TailscaleCall::Release]
    );
}

#[tokio::test]
async fn failed_serve_acquire_keeps_loopback_listener_running_with_degraded_status() {
    let db = SqliteTestDb::new("remote-listener-serve-degraded");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    store.get_or_create().await.expect("settings should mint");
    let port = reserve_loopback_port().await;
    set_configured_port(&db, port);
    let handle = RemoteListenerHandle::new();
    let runner =
        RecordingTailscaleCommandRunner::failing_acquire(TailscaleServeError::CliUnavailable);

    let address = start_listener(&handle, &store, &UnconfiguredTailnetProvider, &runner)
        .await
        .expect("serve degradation must not fail listener start");
    let status = handle.serve_status().await;
    stop_listener(&handle, &store, &runner)
        .await
        .expect("degraded listener should stop");

    assert_eq!(address, SocketAddr::from(([127, 0, 0, 1], port)));
    assert!(!status.active);
    assert!(status.degraded_reason.is_some());
    assert_eq!(
        status.degraded_kind,
        Some(super::RemoteServeDegradedKind::CliUnavailable),
        "PR 1.7 branches on the kind, never on the reason prose (rule 5)"
    );
    assert_eq!(
        runner.calls(),
        vec![TailscaleCall::Acquire(port), TailscaleCall::Release],
        "a Serve start that failed to acquire must clear any mapping an earlier run left \
         behind rather than stay silently tailnet-reachable"
    );
}

/// The doc contract on `start_listener` — "a failed persist releases the socket" — applied to
/// the Serve mapping: an enable that cannot be persisted must not leave :443 forwarding at a
/// listener this process is about to abandon.
#[tokio::test]
async fn a_failed_enable_persist_releases_the_acquired_serve_mapping() {
    let db = SqliteTestDb::new("remote-listener-persist-failure");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    store.get_or_create().await.expect("settings should mint");
    let port = reserve_loopback_port().await;
    set_configured_port(&db, port);
    // Break the store only AFTER the row and port are in place, so the failure lands exactly
    // on `set_enabled` — after the bind and after the Serve acquire.
    db.with_connection(|conn| {
        conn.execute_batch(
            "CREATE TRIGGER refuse_settings_update BEFORE UPDATE ON remote_host_settings
             BEGIN SELECT RAISE(ABORT, 'settings store is unavailable'); END;",
        )
        .expect("refusing trigger should install");
    });
    let handle = RemoteListenerHandle::new();
    let runner = RecordingTailscaleCommandRunner::default();

    let error = start_listener(&handle, &store, &UnconfiguredTailnetProvider, &runner)
        .await
        .expect_err("a failed enable persist must fail the start");

    assert!(matches!(error, RemoteListenerError::Settings(_)));
    assert!(!handle.is_running().await);
    assert_eq!(
        runner.calls(),
        vec![TailscaleCall::Acquire(port), TailscaleCall::Release]
    );
}

#[tokio::test]
async fn stopping_twice_releases_a_serve_mapping_only_once() {
    let db = SqliteTestDb::new("remote-listener-double-stop");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    store.get_or_create().await.expect("settings should mint");
    let port = reserve_loopback_port().await;
    set_configured_port(&db, port);
    let handle = RemoteListenerHandle::new();
    let runner = RecordingTailscaleCommandRunner::default();

    start_listener(&handle, &store, &UnconfiguredTailnetProvider, &runner)
        .await
        .expect("serve listener should start");
    assert!(stop_listener(&handle, &store, &runner)
        .await
        .expect("first stop should succeed"));
    assert!(!stop_listener(&handle, &store, &runner)
        .await
        .expect("second stop should be a no-op"));

    assert_eq!(
        runner.calls(),
        vec![TailscaleCall::Acquire(port), TailscaleCall::Release]
    );
}

#[tokio::test]
async fn tailnet_direct_start_is_refused_while_the_provider_reports_no_tailnet() {
    let db = SqliteTestDb::new("remote-listener-tailnet-refusal");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    store
        .set_exposure_mode(RemoteExposureMode::TailnetDirect)
        .await
        .expect("exposure mode should persist");
    let handle = RemoteListenerHandle::new();
    let runner = RecordingTailscaleCommandRunner::default();

    let error = start_listener(&handle, &store, &UnconfiguredTailnetProvider, &runner)
        .await
        .expect_err("direct exposure must be refused without a validated tailnet address");
    let settings = store
        .get()
        .await
        .expect("settings should read")
        .expect("settings row should exist");

    assert!(matches!(error, RemoteListenerError::Bind(_)));
    assert!(
        !settings.enabled,
        "a refused bind must never persist an enabled listener"
    );
    assert!(!handle.is_running().await);
    assert!(runner.calls().is_empty());
}

#[tokio::test]
async fn successful_tailnet_direct_start_never_changes_serve_configuration() {
    let db = SqliteTestDb::new("remote-listener-tailnet-direct");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    store
        .set_exposure_mode(RemoteExposureMode::TailnetDirect)
        .await
        .expect("exposure mode should persist");
    let handle = RemoteListenerHandle::new();
    let runner = RecordingTailscaleCommandRunner::default();

    let address = start_listener(&handle, &store, &ConfiguredTailnetProvider, &runner)
        .await
        .expect("direct exposure should bind a validated tailnet address");

    assert_eq!(address.ip(), IpAddr::V4(Ipv4Addr::new(100, 101, 102, 103)));
    assert!(runner.calls().is_empty());

    stop_listener(&handle, &store, &runner)
        .await
        .expect("direct listener should stop");

    assert!(runner.calls().is_empty());
}

#[tokio::test]
async fn auto_start_does_nothing_without_an_enabling_settings_row() {
    let absent_db = SqliteTestDb::new("remote-listener-auto-start-absent");
    let absent_store =
        RemoteHostSettingsStore::from_db(DbConnection::from_shared(absent_db.shared_conn()));
    let absent_handle = RemoteListenerHandle::new();
    let disabled_db = SqliteTestDb::new("remote-listener-auto-start-disabled");
    let disabled_store =
        RemoteHostSettingsStore::from_db(DbConnection::from_shared(disabled_db.shared_conn()));
    disabled_store
        .get_or_create()
        .await
        .expect("settings should mint disabled");
    let disabled_handle = RemoteListenerHandle::new();
    let runner = RecordingTailscaleCommandRunner::default();

    let absent = auto_start_if_enabled(
        &absent_handle,
        &absent_store,
        &UnconfiguredTailnetProvider,
        &runner,
    )
    .await
    .expect("an absent row is not an error");
    let disabled = auto_start_if_enabled(
        &disabled_handle,
        &disabled_store,
        &UnconfiguredTailnetProvider,
        &runner,
    )
    .await
    .expect("a disabled row is not an error");

    assert!(absent.is_none());
    assert!(disabled.is_none());
    assert!(!absent_handle.is_running().await);
    assert!(!disabled_handle.is_running().await);
    absent_db.with_connection(|conn| {
        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM remote_host_settings", [], |row| {
                row.get(0)
            })
            .expect("row count should read");
        assert_eq!(row_count, 0, "auto-start must not mint the settings row");
    });
}

#[tokio::test]
async fn auto_start_binds_when_the_persisted_row_enables_the_listener() {
    let db = SqliteTestDb::new("remote-listener-auto-start-enabled");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    store.set_enabled(true).await.expect("settings should mint");
    set_configured_port(&db, reserve_loopback_port().await);
    let handle = RemoteListenerHandle::new();
    let runner = RecordingTailscaleCommandRunner::default();

    let started = auto_start_if_enabled(&handle, &store, &UnconfiguredTailnetProvider, &runner)
        .await
        .expect("an enabled row should auto-start");
    stop_listener(&handle, &store, &runner)
        .await
        .expect("stop should succeed");

    assert!(started.is_some());
}

#[tokio::test]
async fn changing_exposure_mode_persists_while_the_listener_is_stopped() {
    let db = SqliteTestDb::new("remote-listener-exposure-mode");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    let handle = RemoteListenerHandle::new();
    let runner = RecordingTailscaleCommandRunner::default();

    let settings = apply_exposure_mode(
        &handle,
        &store,
        &UnconfiguredTailnetProvider,
        &runner,
        RemoteExposureMode::TailnetDirect,
    )
    .await
    .expect("mode change should persist while stopped");

    assert_eq!(settings.exposure_mode, RemoteExposureMode::TailnetDirect);
    assert!(!handle.is_running().await);
}

#[tokio::test]
async fn a_refused_exposure_mode_change_leaves_remote_access_disabled() {
    let db = SqliteTestDb::new("remote-listener-exposure-mode-refused");
    let store = RemoteHostSettingsStore::from_db(DbConnection::from_shared(db.shared_conn()));
    store.get_or_create().await.expect("settings should mint");
    set_configured_port(&db, reserve_loopback_port().await);
    let handle = RemoteListenerHandle::new();
    let runner = RecordingTailscaleCommandRunner::default();
    start_listener(&handle, &store, &UnconfiguredTailnetProvider, &runner)
        .await
        .expect("serve mode should start");

    let error = apply_exposure_mode(
        &handle,
        &store,
        &UnconfiguredTailnetProvider,
        &runner,
        RemoteExposureMode::TailnetDirect,
    )
    .await
    .expect_err("the restart must be refused without a tailnet address");
    let settings = store
        .get()
        .await
        .expect("settings should read")
        .expect("settings row should exist");

    assert!(matches!(error, RemoteListenerError::Bind(_)));
    assert!(!settings.enabled);
    assert_eq!(settings.exposure_mode, RemoteExposureMode::TailnetDirect);
    assert!(!handle.is_running().await);
}

/// P-16: the :3847 backend stays loopback-pinned; the remote listener never rebinds it.
#[test]
fn the_backend_listener_stays_pinned_to_loopback() {
    assert_eq!(PRODUCTION_BACKEND_PORT, 3847);
    assert!(backend_http_bind_addr().starts_with("127.0.0.1:"));
    assert!(backend_http_base_url().starts_with("http://127.0.0.1:"));
    assert_eq!(
        backend_http_bind_addr(),
        format!("127.0.0.1:{}", backend_http_port())
    );
    assert_ne!(
        backend_http_port(),
        super::settings::DEFAULT_REMOTE_PORT,
        "the remote listener must never share the backend port"
    );
}
