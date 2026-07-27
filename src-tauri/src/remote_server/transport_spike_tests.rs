use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::OnceLock,
};
use tokio::sync::Mutex;
use tower::ServiceExt;

use super::transport_spike::{
    cors_probe_router, start_cors_probe_listener, stop_cors_probe_listener, DebugCorsProbeOrdering,
    DEBUG_CORS_PROBE_ORIGIN,
};
use crate::commands::remote_transport_spike_commands::{
    debug_run_desktop_proxy_stub, DebugDesktopProxyStubInput,
};

static LISTENER_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn listener_test_lock() -> &'static Mutex<()> {
    LISTENER_TEST_LOCK.get_or_init(|| Mutex::new(()))
}

fn preflight_request(origin: &'static str) -> Request<Body> {
    Request::builder()
        .method(Method::OPTIONS)
        .uri("/remote/v1/invoke")
        .header(header::ORIGIN, origin)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, Method::POST.as_str())
        .header(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            "authorization,content-type",
        )
        .body(Body::empty())
        .expect("preflight request should be valid")
}

#[tokio::test]
async fn auth_before_options_rejects_preflight_without_cors_headers() {
    let response = cors_probe_router(DebugCorsProbeOrdering::AuthBeforeOptions)
        .oneshot(preflight_request(DEBUG_CORS_PROBE_ORIGIN))
        .await
        .expect("probe router should respond");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
}

#[tokio::test]
async fn options_before_auth_returns_a_restrictive_preflight_response() {
    let response = cors_probe_router(DebugCorsProbeOrdering::OptionsBeforeAuth)
        .oneshot(preflight_request(DEBUG_CORS_PROBE_ORIGIN))
        .await
        .expect("probe router should respond");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .expect("preflight should name the fixed development origin"),
        DEBUG_CORS_PROBE_ORIGIN
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .expect("preflight should permit the probe method"),
        "POST"
    );
}

#[tokio::test]
async fn options_before_auth_rejects_an_unlisted_origin_without_cors_headers() {
    let unlisted_origin = "https://unlisted.example";
    let response = cors_probe_router(DebugCorsProbeOrdering::OptionsBeforeAuth)
        .oneshot(preflight_request(unlisted_origin))
        .await
        .expect("probe router should respond");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
}

#[tokio::test]
async fn loopback_probe_binds_an_ephemeral_ipv4_port_and_stops_once() {
    let _guard = listener_test_lock().lock().await;
    let endpoint = start_cors_probe_listener(DebugCorsProbeOrdering::OptionsBeforeAuth)
        .await
        .expect("loopback probe should bind");
    let address: SocketAddr = endpoint
        .base_url
        .strip_prefix("http://")
        .expect("endpoint should use HTTP")
        .parse()
        .expect("endpoint should contain a socket address");
    let stopped = stop_cors_probe_listener().expect("probe listener should stop");
    let stopped_again = stop_cors_probe_listener().expect("stop should be idempotent");

    assert_eq!(address.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_ne!(address.port(), 0);
    assert!(stopped);
    assert!(!stopped_again);
}

#[tokio::test]
async fn desktop_proxy_command_uses_the_loopback_fixture_and_reports_its_result() {
    let _guard = listener_test_lock().lock().await;
    let result = debug_run_desktop_proxy_stub(DebugDesktopProxyStubInput {
        ordering: DebugCorsProbeOrdering::OptionsBeforeAuth,
    })
    .await
    .expect("desktop proxy command should reach the fixture");

    assert!(result.fixture_base_url.starts_with("http://127.0.0.1:"));
    assert_eq!(result.request_path, "/remote/v1/invoke");
    assert_eq!(result.status_code, StatusCode::UNAUTHORIZED.as_u16());
    assert_eq!(result.transport, "rustLoopbackHttp");
    let serialized = serde_json::to_value(&result).expect("result should serialize for Tauri IPC");
    assert_eq!(serialized["requestPath"], "/remote/v1/invoke");
    assert_eq!(serialized["statusCode"], StatusCode::UNAUTHORIZED.as_u16());
    assert_eq!(serialized["transport"], "rustLoopbackHttp");
}
