use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::OnceLock,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tower::ServiceExt;

use super::transport_spike::{
    cors_probe_router, start_cors_probe_listener, stop_cors_probe_listener, DebugCorsProbeOrdering,
    DEBUG_CORS_PROBE_ORIGIN,
};
use crate::commands::remote_transport_spike_commands::{
    debug_run_desktop_proxy_stub, debug_start_remote_transport_cors_probe,
    debug_stop_remote_transport_cors_probe, DebugDesktopProxyStubInput,
    DebugStartRemoteTransportCorsProbeInput,
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

struct SocketPreflightResponse {
    status: StatusCode,
    allow_origin: Option<String>,
    allow_methods: Option<String>,
    allow_headers: Option<String>,
    vary: Option<String>,
}

async fn send_preflight_to_listener(
    endpoint: &super::transport_spike::DebugCorsProbeEndpoint,
    origin: &str,
) -> SocketPreflightResponse {
    let address: SocketAddr = endpoint
        .base_url
        .strip_prefix("http://")
        .expect("endpoint should use HTTP")
        .parse()
        .expect("endpoint should contain a socket address");
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("ephemeral probe listener should accept loopback connections");
    let request = format!(
        "OPTIONS /remote/v1/invoke HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nOrigin: {origin}\r\nAccess-Control-Request-Method: POST\r\nAccess-Control-Request-Headers: authorization,content-type\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("socket preflight should reach the listener");

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("socket preflight should receive a complete HTTP response");
    let response = std::str::from_utf8(&response).expect("response should be valid HTTP text");
    let mut lines = response.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .and_then(|status| StatusCode::from_u16(status).ok())
        .expect("response should carry a valid HTTP status");
    let mut parsed = SocketPreflightResponse {
        status,
        allow_origin: None,
        allow_methods: None,
        allow_headers: None,
        vary: None,
    };
    for line in lines.take_while(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().to_string();
        match name.to_ascii_lowercase().as_str() {
            "access-control-allow-origin" => parsed.allow_origin = Some(value),
            "access-control-allow-methods" => parsed.allow_methods = Some(value),
            "access-control-allow-headers" => parsed.allow_headers = Some(value),
            "vary" => parsed.vary = Some(value),
            _ => {}
        }
    }

    parsed
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
async fn options_before_auth_rejects_preflight_without_an_origin() {
    let request = Request::builder()
        .method(Method::OPTIONS)
        .uri("/remote/v1/invoke")
        .body(Body::empty())
        .expect("preflight request should be valid");

    let response = cors_probe_router(DebugCorsProbeOrdering::OptionsBeforeAuth)
        .oneshot(request)
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
async fn starting_a_second_probe_replaces_and_stops_the_first() {
    let _guard = listener_test_lock().lock().await;
    let first = start_cors_probe_listener(DebugCorsProbeOrdering::AuthBeforeOptions)
        .await
        .expect("first probe should bind");
    let second = start_cors_probe_listener(DebugCorsProbeOrdering::OptionsBeforeAuth)
        .await
        .expect("second probe should replace the first");

    assert_ne!(first.base_url, second.base_url);
    assert!(stop_cors_probe_listener().expect("replacement probe should stop"));
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

#[tokio::test]
async fn start_cors_probe_command_returns_a_bound_endpoint() {
    let _guard = listener_test_lock().lock().await;
    let endpoint =
        debug_start_remote_transport_cors_probe(DebugStartRemoteTransportCorsProbeInput {
            ordering: DebugCorsProbeOrdering::OptionsBeforeAuth,
        })
        .await
        .expect("start command should bind the probe");

    assert!(!endpoint.base_url.is_empty());
    assert!(stop_cors_probe_listener().expect("probe listener should stop"));
}

#[tokio::test]
async fn stop_cors_probe_command_reports_a_running_listener_was_stopped() {
    let _guard = listener_test_lock().lock().await;
    start_cors_probe_listener(DebugCorsProbeOrdering::OptionsBeforeAuth)
        .await
        .expect("probe should be running before the stop command");

    assert!(
        debug_stop_remote_transport_cors_probe().expect("stop command should release the probe")
    );
}

#[tokio::test]
async fn actual_listener_auth_before_options_returns_401_without_cors_headers() {
    let _guard = listener_test_lock().lock().await;
    let endpoint = start_cors_probe_listener(DebugCorsProbeOrdering::AuthBeforeOptions)
        .await
        .expect("auth-before-options listener should bind");
    let response = send_preflight_to_listener(&endpoint, DEBUG_CORS_PROBE_ORIGIN).await;
    let stopped = stop_cors_probe_listener().expect("probe listener should stop");

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert!(response.allow_origin.is_none());
    assert!(response.allow_methods.is_none());
    assert!(response.allow_headers.is_none());
    assert!(stopped);
}

#[tokio::test]
async fn actual_listener_options_before_auth_returns_restrictive_cors_for_allowed_origin() {
    let _guard = listener_test_lock().lock().await;
    let endpoint = start_cors_probe_listener(DebugCorsProbeOrdering::OptionsBeforeAuth)
        .await
        .expect("options-before-auth listener should bind");
    let response = send_preflight_to_listener(&endpoint, DEBUG_CORS_PROBE_ORIGIN).await;
    let stopped = stop_cors_probe_listener().expect("probe listener should stop");

    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(
        response.allow_origin.as_deref(),
        Some(DEBUG_CORS_PROBE_ORIGIN)
    );
    assert_eq!(response.allow_methods.as_deref(), Some("POST"));
    assert_eq!(
        response.allow_headers.as_deref(),
        Some("authorization,content-type")
    );
    assert_eq!(response.vary.as_deref(), Some("Origin"));
    assert!(stopped);
}

#[tokio::test]
async fn actual_listener_options_before_auth_denies_an_unlisted_origin_without_cors_headers() {
    let _guard = listener_test_lock().lock().await;
    let endpoint = start_cors_probe_listener(DebugCorsProbeOrdering::OptionsBeforeAuth)
        .await
        .expect("options-before-auth listener should bind");
    let response = send_preflight_to_listener(&endpoint, "https://unlisted.example").await;
    let stopped = stop_cors_probe_listener().expect("probe listener should stop");

    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert!(response.allow_origin.is_none());
    assert!(response.allow_methods.is_none());
    assert!(response.allow_headers.is_none());
    assert!(stopped);
}
