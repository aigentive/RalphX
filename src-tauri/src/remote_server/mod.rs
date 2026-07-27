//! Remote host listener (:3849).
//!
//! Deliberately separate from `http_server` (:3847): that router is trust-by-localhost with no
//! auth middleware and permissive CORS. This one authenticates every route except a two-entry
//! pre-auth allowlist, binds only loopback or a validated tailnet address, and never mounts a
//! :3847 trust-header handler (§2.3, §4.4).

pub mod auth;
pub mod auth_endpoints;
#[cfg(test)]
mod auth_tests;
pub mod capture;
pub mod endpoints;
#[cfg(test)]
mod listener_tests;
pub mod rate_limit;
#[cfg(test)]
mod rate_limit_tests;
pub mod session_registry;
#[cfg(test)]
mod session_registry_tests;
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
    extract::DefaultBodyLimit,
    http::{header, HeaderValue, Method, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use ralphx_remote_protocol::{ErrorCode, ResetReason};
use serde::Serialize;
use tauri::Manager;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex};
use tokio_util::sync::CancellationToken;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::error::AppError;
use crate::remote_server::auth::{
    authenticate_remote_request, enforce_auth_endpoint_rate_limit, strip_trust_headers,
    RemoteAuthContext,
};
use crate::remote_server::auth_endpoints::{
    pair_handler, session_introspection_handler, session_teardown_handler, ws_ticket_handler,
    REMOTE_AUTH_BODY_LIMIT_BYTES,
};
use crate::remote_server::endpoints::{
    environment_descriptor_handler, health_handler, RemoteRouterState,
};
use crate::remote_server::session_registry::RemoteSessionRegistry;
use crate::remote_server::settings::{
    effective_remote_port, resolve_bind_address, RemoteBindError, RemoteExposureMode,
    RemoteHostSettings, RemoteHostSettingsStore, TailnetSelfAddressProvider,
    UnconfiguredTailnetProvider,
};

pub(crate) const DESCRIPTOR_PATH: &str = "/.well-known/ralphx/environment";
pub(crate) const PAIR_PATH: &str = "/remote/v1/auth/pair";
pub(crate) const WS_TICKET_PATH: &str = "/remote/v1/auth/ws-ticket";
pub(crate) const SESSION_PATH: &str = "/remote/v1/session";
pub(crate) const HEALTH_PATH: &str = "/health";

/// Routes reachable before the bearer check.
///
/// Exactly two: discovery and pairing. Everything else — including `/health` — runs behind
/// [`authenticate_remote_request`]; there is no zero-devices bootstrap pass (§4.4, A-2).
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
}

/// Process-owned handle for the single remote listener.
///
/// Registered as Tauri managed state so the enable/disable commands and startup auto-start
/// share one listener rather than racing separate binds.
#[derive(Clone)]
pub(crate) struct RemoteListenerHandle {
    active: Arc<Mutex<Option<ActiveRemoteListener>>>,
    /// Lives on the handle, not inside the router, so it survives listener restarts and is
    /// reachable from the host-local revoke commands (§4.4).
    sessions: RemoteSessionRegistry,
}

impl RemoteListenerHandle {
    pub(crate) fn new() -> Self {
        Self {
            active: Arc::new(Mutex::new(None)),
            sessions: RemoteSessionRegistry::new(),
        }
    }

    /// The process-wide live-session registry.
    pub(crate) fn sessions(&self) -> &RemoteSessionRegistry {
        &self.sessions
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
            PAIR_PATH,
            post(pair_handler)
                .options(remote_preflight_handler)
                .layer(DefaultBodyLimit::max(REMOTE_AUTH_BODY_LIMIT_BYTES)),
        )
        .route(
            WS_TICKET_PATH,
            post(ws_ticket_handler)
                .options(remote_preflight_handler)
                .layer(DefaultBodyLimit::max(REMOTE_AUTH_BODY_LIMIT_BYTES)),
        )
        .route(
            SESSION_PATH,
            get(session_introspection_handler)
                .delete(session_teardown_handler)
                .options(remote_preflight_handler),
        )
        .route(
            HEALTH_PATH,
            get(health_handler).options(remote_preflight_handler),
        )
        .fallback(remote_fallback_handler)
        // Layers apply outermost-last: trust headers are stripped before anything else
        // runs, then pre-auth flood control, then the bearer check.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            authenticate_remote_request,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            enforce_auth_endpoint_rate_limit,
        ))
        .layer(middleware::from_fn(strip_trust_headers))
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

pub(crate) fn remote_error_response(
    status: StatusCode,
    code: ErrorCode,
    message: impl Into<String>,
) -> Response {
    (
        status,
        Json(RemoteErrorBody {
            code,
            message: message.into(),
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

    store.set_enabled(true).await?;

    let shutdown = CancellationToken::new();
    let serve_shutdown = shutdown.clone();
    let (stopped_tx, stopped) = oneshot::channel();
    let auth =
        RemoteAuthContext::from_db(store.db(), handle.sessions.clone(), settings.exposure_mode);
    let router = remote_router(RemoteRouterState::new(
        settings.environment_id.as_str(),
        auth,
    ));

    tauri::async_runtime::spawn(async move {
        // Connect info is what lets direct-tailnet mode key rate limiting on the real peer;
        // under Serve the address is loopback and deliberately ignored (§4.4).
        match axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
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
) -> Result<bool, RemoteListenerError> {
    let mut active = handle.active.lock().await;
    // Durable intent first, then the teardown effect: a caller that observes `enabled =
    // false` must never find a live session still attached (§4.4 teardown order).
    store.set_enabled(false).await?;
    let torn_down = handle.sessions.kill_all(ResetReason::HostDisabled);
    if torn_down > 0 {
        tracing::info!(
            sessions = torn_down,
            "Remote listener disable tore down live sessions"
        );
    }
    let Some(listener) = active.take() else {
        tracing::debug!("Remote listener stop requested while it was not running");
        return Ok(false);
    };

    tracing::info!(address = %listener.bind_address, "Remote listener stopping");
    listener.shutdown.cancel();
    // Waiting for the serve task guarantees the port is released before the lock is released,
    // so a subsequent enable can re-acquire it.
    let _ = listener.stopped.await;
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
    exposure_mode: RemoteExposureMode,
) -> Result<RemoteHostSettings, RemoteListenerError> {
    let was_running = handle.is_running().await;
    if was_running {
        stop_listener(handle, store).await?;
    }

    let settings = store.set_exposure_mode(exposure_mode).await?;
    if !was_running {
        return Ok(settings);
    }

    match start_listener(handle, store, provider).await {
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
) -> Result<Option<SocketAddr>, RemoteListenerError> {
    let Some(settings) = store.get().await? else {
        tracing::debug!("Remote host settings are absent; remote listener stays off");
        return Ok(None);
    };
    if !settings.enabled {
        tracing::debug!("Remote host mode is disabled; remote listener stays off");
        return Ok(None);
    }
    start_listener(handle, store, provider).await.map(Some)
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

    match auto_start_if_enabled(&handle, &store, &UnconfiguredTailnetProvider).await {
        Ok(Some(address)) => {
            tracing::info!(%address, "Remote listener auto-started from persisted settings");
        }
        Ok(None) => {}
        Err(error) => tracing::error!(%error, "Remote listener auto-start failed"),
    }
}
