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
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Fixed local development origin used by the browser CORS ordering probe.
pub const DEBUG_CORS_PROBE_ORIGIN: &str = "http://127.0.0.1:1420";
const LOOPBACK_EPHEMERAL_BIND_ADDR: &str = "127.0.0.1:0";

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

    Ok(DebugCorsProbeEndpoint {
        base_url: format!("http://{address}"),
        ordering,
    })
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
