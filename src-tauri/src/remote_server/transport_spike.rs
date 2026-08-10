//! Debug-only loopback fixture for the PR 0.3 direct-browser CORS probe.
//!
//! This fixture intentionally models ordering only. It has no remote-listener routes, credentials,
//! pairing state, or production auth behavior.

use axum::{
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::options,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Fixed local development origin used by the browser CORS ordering probe.
pub const DEBUG_CORS_PROBE_ORIGIN: &str = "http://127.0.0.1:1420";
const LOOPBACK_EPHEMERAL_BIND_ADDR: &str = "127.0.0.1:0";
const DEBUG_PROXY_REQUEST_PATH: &str = "/remote/v1/invoke";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DebugCorsProbeOrdering {
    AuthBeforeOptions,
    OptionsBeforeAuth,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugCorsProbeEndpoint {
    pub base_url: String,
    pub ordering: DebugCorsProbeOrdering,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugDesktopProxyStubResult {
    pub fixture_base_url: String,
    pub request_path: &'static str,
    pub status_code: u16,
    pub transport: &'static str,
}

struct DebugCorsProbeListener {
    shutdown: CancellationToken,
}

static ACTIVE_CORS_PROBE: OnceLock<Mutex<Option<DebugCorsProbeListener>>> = OnceLock::new();

fn active_cors_probe() -> &'static Mutex<Option<DebugCorsProbeListener>> {
    ACTIVE_CORS_PROBE.get_or_init(|| Mutex::new(None))
}

/// Starts a loopback-only, ephemeral CORS ordering fixture for manual browser probes.
///
/// The caller may use the returned endpoint only with `DEBUG_CORS_PROBE_ORIGIN`; it models a
/// preflight ordering failure/success without accepting bearer or pairing credentials.
///
/// # Errors
///
/// Returns an error when the OS cannot bind a loopback socket or the debug listener state is
/// poisoned.
pub async fn start_cors_probe_listener(
    ordering: DebugCorsProbeOrdering,
) -> Result<DebugCorsProbeEndpoint, String> {
    Ok(start_cors_probe_listener_with_address(ordering).await?.0)
}

async fn start_cors_probe_listener_with_address(
    ordering: DebugCorsProbeOrdering,
) -> Result<(DebugCorsProbeEndpoint, std::net::SocketAddr), String> {
    let listener = TcpListener::bind(LOOPBACK_EPHEMERAL_BIND_ADDR)
        .await
        .map_err(|error| format!("Failed to bind debug CORS probe listener: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("Failed to read debug CORS probe listener address: {error}"))?;
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();

    tauri::async_runtime::spawn(async move {
        if let Err(error) = axum::serve(listener, cors_probe_router(ordering))
            .with_graceful_shutdown(server_shutdown.cancelled_owned())
            .await
        {
            tracing::warn!(?error, "debug CORS probe listener stopped unexpectedly");
        }
    });

    let previous = {
        let mut active = active_cors_probe()
            .lock()
            .map_err(|_| "Debug CORS probe listener state is unavailable".to_string())?;
        active.replace(DebugCorsProbeListener { shutdown })
    };
    if let Some(previous) = previous {
        previous.shutdown.cancel();
    }

    Ok((
        DebugCorsProbeEndpoint {
            base_url: format!("http://{address}"),
            ordering,
        },
        address,
    ))
}

/// Stops the current debug CORS probe listener, if one is running.
pub fn stop_cors_probe_listener() -> Result<bool, String> {
    let listener = active_cors_probe()
        .lock()
        .map_err(|_| "Debug CORS probe listener state is unavailable".to_string())?
        .take();
    if let Some(listener) = listener {
        listener.shutdown.cancel();
        return Ok(true);
    }
    Ok(false)
}

/// Performs a fixed remote-shaped HTTP request from the Rust side of the debug fixture.
///
/// This models the desktop transport boundary only: the target is the fixture's own loopback
/// address, and no caller can provide a host, bearer, pairing code, or request path.
///
/// # Errors
///
/// Returns an error if the fixture cannot bind, the Rust-side request cannot complete, or the
/// fixture cannot be stopped afterward.
pub async fn run_desktop_proxy_stub(
    ordering: DebugCorsProbeOrdering,
) -> Result<DebugDesktopProxyStubResult, String> {
    let (endpoint, address) = start_cors_probe_listener_with_address(ordering).await?;
    let request_result = request_loopback_probe(address).await;
    let stop_result = stop_cors_probe_listener();

    let status_code = request_result?;
    if !stop_result? {
        return Err("Debug CORS probe listener was not active after the proxy request".to_string());
    }

    Ok(DebugDesktopProxyStubResult {
        fixture_base_url: endpoint.base_url,
        request_path: DEBUG_PROXY_REQUEST_PATH,
        status_code,
        transport: "rustLoopbackHttp",
    })
}

async fn request_loopback_probe(address: std::net::SocketAddr) -> Result<u16, String> {
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .map_err(|error| {
            format!("Debug desktop proxy could not reach loopback fixture: {error}")
        })?;
    let request = format!(
        "POST {DEBUG_PROXY_REQUEST_PATH} HTTP/1.0\r\nHost: {address}\r\nContent-Length: 0\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| {
            format!("Debug desktop proxy could not write to loopback fixture: {error}")
        })?;

    let mut response = [0_u8; 1024];
    let read = stream.read(&mut response).await.map_err(|error| {
        format!("Debug desktop proxy could not read loopback response: {error}")
    })?;
    std::str::from_utf8(&response[..read])
        .ok()
        .and_then(|response| response.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| "Debug desktop proxy received an invalid loopback HTTP response".to_string())
}

pub(crate) fn cors_probe_router(ordering: DebugCorsProbeOrdering) -> Router {
    match ordering {
        DebugCorsProbeOrdering::AuthBeforeOptions => {
            Router::new().fallback(unauthorized_probe_response)
        }
        DebugCorsProbeOrdering::OptionsBeforeAuth => Router::new().route(
            "/*path",
            options(preflight_probe_response).fallback(unauthorized_probe_response),
        ),
    }
}

async fn unauthorized_probe_response() -> StatusCode {
    // Deliberately fixed: this is a CORS-ordering fixture, not Phase 1 authentication.
    StatusCode::UNAUTHORIZED
}

async fn preflight_probe_response(headers: HeaderMap) -> Response {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if origin.as_bytes() != DEBUG_CORS_PROBE_ORIGIN.as_bytes() {
        return StatusCode::FORBIDDEN.into_response();
    }

    (
        StatusCode::NO_CONTENT,
        [
            (
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static(DEBUG_CORS_PROBE_ORIGIN),
            ),
            (
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("POST"),
            ),
            (
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("authorization,content-type"),
            ),
            (header::VARY, HeaderValue::from_static("Origin")),
        ],
    )
        .into_response()
}
