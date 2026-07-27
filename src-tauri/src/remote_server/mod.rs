//! Remote host listener (:3849).
//!
//! Deliberately separate from `http_server` (:3847): that router is trust-by-localhost with no
//! auth middleware and permissive CORS. This one authenticates every route except a two-entry
//! pre-auth allowlist, binds only loopback or a validated tailnet address, and never mounts a
//! :3847 trust-header handler (§2.3, §4.4).

pub mod capture;
pub mod endpoints;
#[cfg(test)]
mod endpoints_tests;
#[cfg(test)]
mod listener_tests;
pub mod settings;
#[cfg(test)]
mod settings_tests;
#[cfg(debug_assertions)]
pub mod transport_spike;
#[cfg(all(test, debug_assertions))]
mod transport_spike_tests;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::Request,
    http::{header, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use ralphx_remote_protocol::ErrorCode;
use serde::Serialize;
use tauri::Manager;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex};
use tokio_util::sync::CancellationToken;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::error::AppError;
use crate::infrastructure::tailscale::{
    RealTailscaleCommandRunner, TailscaleCommandRunner, TailscaleSelfAddressProvider,
    TailscaleServeError,
};
use crate::remote_server::endpoints::{
    environment_descriptor_handler, health_handler, RemoteRouterState,
};
use crate::remote_server::settings::{
    effective_remote_port, resolve_bind_address, RemoteBindError, RemoteExposureMode,
    RemoteHostSettings, RemoteHostSettingsStore, TailnetSelfAddressProvider,
};

pub(crate) const DESCRIPTOR_PATH: &str = "/.well-known/ralphx/environment";
pub(crate) const PAIR_PATH: &str = "/remote/v1/auth/pair";
pub(crate) const HEALTH_PATH: &str = "/health";

/// Routes reachable before the bearer check.
///
/// Exactly two: discovery and pairing. PR 1.2 replaces [`remote_auth_slot`]'s body with real
/// bearer verification but must keep this allowlist unchanged (§4.4, A-2).
pub(crate) const PRE_AUTH_ALLOWLIST: &[&str] = &[DESCRIPTOR_PATH, PAIR_PATH];

/// Origins the shipped app itself uses.
pub(crate) const PRODUCTION_APP_ORIGINS: &[&str] = &["tauri://localhost"];

/// Dev-server origins, admitted only in debug builds.
pub(crate) const DEVELOPMENT_APP_ORIGINS: &[&str] = &[
    "http://127.0.0.1:1420",
    "http://localhost:1420",
    "http://127.0.0.1:5173",
    "http://localhost:5173",
];

/// The exact origin list the remote CORS layer admits.
///
/// Unlike :3847 (`http_server/mod.rs` `allow_origin(Any)`), the remote router never admits an
/// arbitrary origin (C-15).
pub(crate) fn allowed_app_origins() -> Vec<&'static str> {
    let mut origins = PRODUCTION_APP_ORIGINS.to_vec();
    if cfg!(debug_assertions) {
        origins.extend_from_slice(DEVELOPMENT_APP_ORIGINS);
    }
    origins
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

/// Typed failure modes for the listener lifecycle.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RemoteListenerError {
    #[error(transparent)]
    Bind(#[from] RemoteBindError),
    #[error("remote listener could not bind {address}: {source}")]
    Socket {
        address: SocketAddr,
        source: std::io::Error,
    },
    #[error(transparent)]
    Settings(#[from] AppError),
}

struct ActiveRemoteListener {
    shutdown: CancellationToken,
    stopped: oneshot::Receiver<()>,
    bind_address: SocketAddr,
    serve: RemoteServeStatus,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RemoteServeStatus {
    pub(crate) active: bool,
    pub(crate) degraded_reason: Option<String>,
}

/// Process-owned handle for the single remote listener.
///
/// Registered as Tauri managed state so the enable/disable commands and startup auto-start
/// share one listener rather than racing separate binds.
#[derive(Clone)]
pub(crate) struct RemoteListenerHandle {
    active: Arc<Mutex<Option<ActiveRemoteListener>>>,
}

impl RemoteListenerHandle {
    pub(crate) fn new() -> Self {
        Self {
            active: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) async fn bound_address(&self) -> Option<SocketAddr> {
        self.active
            .lock()
            .await
            .as_ref()
            .map(|listener| listener.bind_address)
    }

    pub(crate) async fn is_running(&self) -> bool {
        self.bound_address().await.is_some()
    }

    pub(crate) async fn serve_status(&self) -> RemoteServeStatus {
        self.active
            .lock()
            .await
            .as_ref()
            .map(|listener| listener.serve.clone())
            .unwrap_or_default()
    }
}

impl Default for RemoteListenerHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the process-wide listener handle, registering it on first use.
pub(crate) fn remote_listener_handle(app_handle: &tauri::AppHandle) -> RemoteListenerHandle {
    if let Some(existing) = app_handle.try_state::<RemoteListenerHandle>() {
        return existing.inner().clone();
    }
    let created = RemoteListenerHandle::new();
    if app_handle.manage(created.clone()) {
        created
    } else {
        app_handle.state::<RemoteListenerHandle>().inner().clone()
    }
}

/// Full remote router: routes, fail-closed auth slot, restrictive CORS.
pub(crate) fn remote_router(state: RemoteRouterState) -> Router {
    authenticated_remote_routes(state).layer(remote_cors_layer())
}

/// The remote route stack without the CORS layer.
///
/// Exposed so tests can prove the auth slot itself lets `OPTIONS` through instead of relying
/// on the CORS layer short-circuiting preflight ahead of it.
pub(crate) fn authenticated_remote_routes(state: RemoteRouterState) -> Router {
    Router::new()
        .route(
            DESCRIPTOR_PATH,
            get(environment_descriptor_handler).options(remote_preflight_handler),
        )
        .route(
            HEALTH_PATH,
            get(health_handler).options(remote_preflight_handler),
        )
        .fallback(remote_fallback_handler)
        .layer(middleware::from_fn(remote_auth_slot))
        .with_state(state)
}

fn remote_cors_layer() -> CorsLayer {
    let origins = allowed_app_origins()
        .into_iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect::<Vec<_>>();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

/// Global fail-closed middleware slot.
///
/// PR 1.2 lands bearer extraction, hashing, device lookup, and header stripping here. Until
/// then every non-allowlisted route is refused, so no route can accidentally ship unauthenticated.
async fn remote_auth_slot(request: Request, next: Next) -> Response {
    if request.method() == Method::OPTIONS {
        return next.run(request).await;
    }
    if PRE_AUTH_ALLOWLIST.contains(&request.uri().path()) {
        return next.run(request).await;
    }
    remote_error_response(
        StatusCode::UNAUTHORIZED,
        ErrorCode::RemoteUnauthorized,
        "Remote authentication is required.",
    )
}

async fn remote_preflight_handler() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

async fn remote_fallback_handler(method: Method) -> Response {
    if method == Method::OPTIONS {
        return StatusCode::NO_CONTENT.into_response();
    }
    remote_error_response(
        StatusCode::NOT_FOUND,
        ErrorCode::RemoteCommandUnavailable,
        "This remote route is not available.",
    )
}

fn remote_error_response(status: StatusCode, code: ErrorCode, message: &'static str) -> Response {
    (
        status,
        Json(RemoteErrorBody {
            code,
            message: message.to_string(),
        }),
    )
        .into_response()
}

/// Binds and serves the remote listener, persisting the enablement flag once the bind succeeds.
///
/// Ordering is deliberate: refuse → bind → persist → spawn. A refused or failed bind never
/// leaves `enabled = true` behind, and a failed persist releases the socket.
pub(crate) async fn start_listener(
    handle: &RemoteListenerHandle,
    store: &RemoteHostSettingsStore,
    provider: &dyn TailnetSelfAddressProvider,
    tailscale: &dyn TailscaleCommandRunner,
) -> Result<SocketAddr, RemoteListenerError> {
    let mut active = handle.active.lock().await;
    if let Some(listener) = active.as_ref() {
        return Ok(listener.bind_address);
    }

    let settings = store.get_or_create().await?;
    let port = effective_remote_port(settings.port);
    let bind_address = match resolve_bind_address(settings.exposure_mode, port, provider).await {
        Ok(address) => address,
        Err(error) => {
            tracing::error!(
                %error,
                exposure_mode = ?settings.exposure_mode,
                "Remote listener bind refused"
            );
            return Err(RemoteListenerError::Bind(error));
        }
    };

    let listener = match TcpListener::bind(bind_address).await {
        Ok(listener) => listener,
        Err(source) => {
            tracing::error!(address = %bind_address, %source, "Remote listener failed to bind");
            return Err(RemoteListenerError::Socket {
                address: bind_address,
                source,
            });
        }
    };
    let bound_address = match listener.local_addr() {
        Ok(address) => address,
        Err(source) => {
            tracing::error!(address = %bind_address, %source, "Remote listener bind address unreadable");
            return Err(RemoteListenerError::Socket {
                address: bind_address,
                source,
            });
        }
    };

    let serve = if settings.exposure_mode == RemoteExposureMode::Serve {
        match tailscale.run_serve_acquire(bound_address.port()).await {
            Ok(()) => RemoteServeStatus {
                active: true,
                degraded_reason: None,
            },
            Err(error) => {
                let degraded_reason = tailscale_serve_degraded_reason(&error);
                tracing::warn!(%error, address = %bound_address, "Tailscale Serve unavailable; remote listener remains loopback-only");
                RemoteServeStatus {
                    active: false,
                    degraded_reason: Some(degraded_reason),
                }
            }
        }
    } else {
        RemoteServeStatus::default()
    };

    if let Err(error) = store.set_enabled(true).await {
        if serve.active {
            release_serve_best_effort(tailscale, bound_address).await;
        }
        return Err(error.into());
    }

    let shutdown = CancellationToken::new();
    let serve_shutdown = shutdown.clone();
    let (stopped_tx, stopped) = oneshot::channel();
    let router = remote_router(RemoteRouterState::new(settings.environment_id.as_str()));

    tauri::async_runtime::spawn(async move {
        match axum::serve(listener, router)
            .with_graceful_shutdown(serve_shutdown.cancelled_owned())
            .await
        {
            Ok(()) => tracing::info!("Remote listener shut down cleanly"),
            Err(error) => tracing::error!(%error, "Remote listener stopped unexpectedly"),
        }
        let _ = stopped_tx.send(());
    });

    *active = Some(ActiveRemoteListener {
        shutdown,
        stopped,
        bind_address: bound_address,
        serve,
    });
    tracing::info!(
        address = %bound_address,
        exposure_mode = ?settings.exposure_mode,
        "Remote listener started"
    );
    Ok(bound_address)
}

/// Persists the disabled flag, then gracefully drains and releases the port.
///
/// Returns whether a listener was actually running.
pub(crate) async fn stop_listener(
    handle: &RemoteListenerHandle,
    store: &RemoteHostSettingsStore,
    tailscale: &dyn TailscaleCommandRunner,
) -> Result<bool, RemoteListenerError> {
    let mut active = handle.active.lock().await;
    store.set_enabled(false).await?;
    let Some(listener) = active.take() else {
        tracing::debug!("Remote listener stop requested while it was not running");
        return Ok(false);
    };

    tracing::info!(address = %listener.bind_address, "Remote listener stopping");
    listener.shutdown.cancel();
    // Waiting for the serve task guarantees the port is released before the lock is released,
    // so a subsequent enable can re-acquire it.
    let _ = listener.stopped.await;
    if listener.serve.active {
        release_serve_best_effort(tailscale, listener.bind_address).await;
    }
    tracing::info!(address = %listener.bind_address, "Remote listener stopped");
    Ok(true)
}

/// Persists a new exposure mode, restarting a running listener so the bind policy re-applies.
///
/// A restart that gets refused leaves remote access disabled rather than silently listening on
/// the previous address.
pub(crate) async fn apply_exposure_mode(
    handle: &RemoteListenerHandle,
    store: &RemoteHostSettingsStore,
    provider: &dyn TailnetSelfAddressProvider,
    tailscale: &dyn TailscaleCommandRunner,
    exposure_mode: RemoteExposureMode,
) -> Result<RemoteHostSettings, RemoteListenerError> {
    let was_running = handle.is_running().await;
    if was_running {
        stop_listener(handle, store, tailscale).await?;
    }

    let settings = store.set_exposure_mode(exposure_mode).await?;
    if !was_running {
        return Ok(settings);
    }

    match start_listener(handle, store, provider, tailscale).await {
        Ok(_) => Ok(store.get_or_create().await?),
        Err(error) => {
            tracing::error!(
                %error,
                ?exposure_mode,
                "Remote listener could not restart after an exposure-mode change; remote access left disabled"
            );
            Err(error)
        }
    }
}

/// Startup auto-start. Never mints the settings row: an absent row means nothing listens.
pub(crate) async fn auto_start_if_enabled(
    handle: &RemoteListenerHandle,
    store: &RemoteHostSettingsStore,
    provider: &dyn TailnetSelfAddressProvider,
    tailscale: &dyn TailscaleCommandRunner,
) -> Result<Option<SocketAddr>, RemoteListenerError> {
    let Some(settings) = store.get().await? else {
        tracing::debug!("Remote host settings are absent; remote listener stays off");
        return Ok(None);
    };
    if !settings.enabled {
        tracing::debug!("Remote host mode is disabled; remote listener stays off");
        return Ok(None);
    }
    start_listener(handle, store, provider, tailscale)
        .await
        .map(Some)
}

/// Startup hook, invoked from the same setup phase that calls `start_server_boot`.
///
/// A bind failure here is logged and left alone: the persisted intent stays enabled so a
/// transient port conflict does not silently turn remote access off.
pub(crate) async fn auto_start_remote_listener_from_handle(app_handle: &tauri::AppHandle) {
    let Some(state) = app_handle.try_state::<crate::AppState>() else {
        tracing::warn!("AppState is unavailable; skipping remote listener auto-start");
        return;
    };
    let store = RemoteHostSettingsStore::from_db(state.db.clone());
    let handle = remote_listener_handle(app_handle);

    match auto_start_if_enabled(
        &handle,
        &store,
        &TailscaleSelfAddressProvider,
        &RealTailscaleCommandRunner,
    )
    .await
    {
        Ok(Some(address)) => {
            tracing::info!(%address, "Remote listener auto-started from persisted settings");
        }
        Ok(None) => {}
        Err(error) => tracing::error!(%error, "Remote listener auto-start failed"),
    }
}

fn tailscale_serve_degraded_reason(error: &TailscaleServeError) -> String {
    match error {
        TailscaleServeError::CliUnavailable
        | TailscaleServeError::Launch(_)
        | TailscaleServeError::Timeout
        | TailscaleServeError::Output(_)
        | TailscaleServeError::Exit(_) => error.to_string(),
    }
}

async fn release_serve_best_effort(
    tailscale: &dyn TailscaleCommandRunner,
    bind_address: SocketAddr,
) {
    if let Err(error) = tailscale.run_serve_release().await {
        tracing::warn!(%error, address = %bind_address, "Tailscale Serve release failed");
    }
}
