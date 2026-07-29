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
mod endpoints_tests;
// --- PR 3.2: two-instance chat-stream validation harness (shared fixture, extended by 3.4) ---
//
// `feature = "test-utils"`, not `cfg(test)`: the fixture is consumed by PR 3.4's integration
// test binary, which cannot see `cfg(test)` items, and it needs `tauri::test::MockRuntime`
// — which is exactly what `test-utils = ["tauri/test"]` turns on.
#[cfg(feature = "test-utils")]
pub mod harness;
#[cfg(all(test, feature = "test-utils"))]
mod harness_tests;
// --- end PR 3.2 block ---
#[cfg(test)]
mod listener_tests;
pub mod rate_limit;
#[cfg(test)]
mod rate_limit_tests;
pub mod retention;
pub mod sequencer;
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
pub mod ws;

// --- PR 1.3: invoke facade, capability ledger, authority audit (one contiguous block) ---
#[cfg(test)]
pub mod authority_audit;
#[cfg(test)]
mod authority_audit_tests;
pub mod capability_ledger;
#[cfg(test)]
mod capability_ledger_tests;
pub mod invoke;
#[cfg(test)]
mod invoke_tests;
pub mod registry;
#[cfg(test)]
mod registry_tests;
// --- end PR 1.3 block ---

// --- PR 1.5: mutating surface — generated scope suite (one contiguous block) ---
#[cfg(test)]
mod scope_suite_tests;
// --- end PR 1.5 block ---

// --- PR 1.5-C: request idempotency + attachment ingress (one contiguous block) ---
pub mod attachments;
#[cfg(test)]
mod attachments_tests;
pub mod dedup;
#[cfg(test)]
mod dedup_tests;
// --- end PR 1.5-C block ---

// --- PR 1.5-B: curated fetch-route remount (one contiguous block) ---
pub mod fetch_remount;
#[cfg(test)]
mod fetch_remount_tests;
// --- end PR 1.5-B block ---

use std::net::SocketAddr;
use std::path::PathBuf;
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

use crate::domain::repositories::RemoteSessionRepository;
use crate::error::AppError;
use crate::infrastructure::sqlite::{
    SqliteRemoteAccessRepository, SqliteRemoteEventLogRepository,
    SqliteRemoteRequestDedupRepository,
};
use crate::infrastructure::tailscale::{
    RealTailscaleCommandRunner, TailscaleCommandRunner, TailscaleSelfAddressProvider,
    TailscaleServeError,
};
use crate::remote_server::attachments::{
    fetch_handler as attachment_fetch_handler, upload_handler as attachment_upload_handler,
    RemoteAttachmentContext, ATTACHMENT_FETCH_PATH, ATTACHMENT_UPLOAD_PATH,
    REMOTE_ATTACHMENT_MAX_BYTES,
};
use crate::remote_server::auth::{
    authenticate_remote_request, enforce_auth_endpoint_rate_limit, strip_trust_headers,
    RemoteAuthContext,
};
use crate::remote_server::auth_endpoints::{
    pair_handler, self_revoke_handler, session_introspection_handler, session_teardown_handler,
    ws_ticket_handler, REMOTE_AUTH_BODY_LIMIT_BYTES,
};
use crate::remote_server::dedup::{RemoteDedupState, REMOTE_INVOKE_BODY_LIMIT_BYTES};
use crate::remote_server::endpoints::{
    environment_descriptor_handler, health_handler, RemoteRouterState,
};
use crate::remote_server::invoke::{RemoteInvokeDispatcher, TauriRemoteInvokeDispatcher};
use crate::remote_server::retention::RetentionLeaseRegistry;
use crate::remote_server::session_registry::RemoteSessionRegistry;
use crate::remote_server::settings::{
    effective_remote_port, resolve_bind_address, RemoteBindError, RemoteExposureMode,
    RemoteHostSettings, RemoteHostSettingsStore, TailnetSelfAddressProvider,
};

pub(crate) const DESCRIPTOR_PATH: &str = "/.well-known/ralphx/environment";
pub(crate) const PAIR_PATH: &str = "/remote/v1/auth/pair";
pub(crate) const WS_TICKET_PATH: &str = "/remote/v1/auth/ws-ticket";
/// Bearer-authenticated self-revocation, called by the client's staged remove/re-pair
/// machines (§6.1 "host revoke (best-effort) → Keychain delete → row delete", P-27).
pub(crate) const REVOKE_PATH: &str = "/remote/v1/auth/revoke";
pub(crate) const SESSION_PATH: &str = "/remote/v1/session";
pub(crate) const INVOKE_PATH: &str = "/remote/v1/invoke";
pub(crate) const HEALTH_PATH: &str = "/health";

/// Routes reachable before the bearer check.
///
/// Exactly two: discovery and pairing. Everything else — including `/health` — runs behind
/// [`authenticate_remote_request`]; there is no zero-devices bootstrap pass (§4.4, A-2).
pub(crate) const PRE_AUTH_ALLOWLIST: &[&str] = &[DESCRIPTOR_PATH, PAIR_PATH];

/// Routes that authenticate themselves instead of presenting a bearer token.
///
/// Exactly one: the WS upgrade. **This is not a pre-auth route** — it is device-bound
/// single-use-ticket auth performed *before* the upgrade, because a browser cannot attach an
/// `Authorization` header to a WebSocket (§3.2). It is kept separate from
/// [`PRE_AUTH_ALLOWLIST`] so the two-entry pre-auth guarantee (P-1) stays literally true and a
/// future addition here has to be argued on its own terms.
pub(crate) const TICKET_AUTH_ALLOWLIST: &[&str] = &[ws::WS_EVENTS_PATH];

/// Origins the shipped app itself uses.
pub(crate) const PRODUCTION_APP_ORIGINS: &[&str] = &["tauri://localhost"];

/// Bounded durable capture queue. Overflow means an epoch roll, not backpressure (§3.4 #3) —
/// blocking here would block whichever thread emitted the event.
pub(crate) const REMOTE_CAPTURE_QUEUE_CAPACITY: usize = 1_024;

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

/// Serve exposure as observed **at listener start**.
///
/// Snapshot semantics, deliberately: nothing re-probes `tailscaled` afterwards, so `active`
/// stays `true` for the listener's lifetime even if the daemon dies or the user runs
/// `tailscale serve off` by hand. PR 1.7's pane must treat it as "what the last start
/// achieved" and revalidate through a reachability probe before claiming a host is reachable.
#[derive(Clone, Debug, Default)]
pub(crate) struct RemoteServeStatus {
    pub(crate) active: bool,
    pub(crate) degraded_reason: Option<String>,
    /// Machine-readable companion to `degraded_reason` (rule 5: no string matching). PR 1.7
    /// branches on this to say "install Tailscale" vs "enable Serve" instead of grepping
    /// prose.
    pub(crate) degraded_kind: Option<RemoteServeDegradedKind>,
}

/// Why Serve is not active, as a value the UI can branch on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteServeDegradedKind {
    /// The `tailscale` CLI could not be resolved — the "install Tailscale" case.
    CliUnavailable,
    /// The CLI exists but could not be launched.
    LaunchFailed,
    /// The command hung past the timeout.
    Timeout,
    /// The command ran and refused (Serve/HTTPS not enabled on the tailnet, not logged in).
    CommandFailed,
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
    /// The durable stream, installed once at app setup.
    ///
    /// A `OnceLock` rather than a mutable slot because the epoch's per-boot guarantee depends on
    /// there being exactly one sequencer per process: a second install would mint a second epoch
    /// and silently fork the numbering. Listener stop/start deliberately does not clear it —
    /// capture keeps recording while the listener is off (§5.2, P-15, P-23).
    stream: Arc<std::sync::OnceLock<sequencer::RemoteStreamHandle>>,
    /// The host-local bus the WS session emits `remote:session_connected/closed` on.
    lifecycle: Arc<std::sync::OnceLock<Arc<dyn ws::SessionLifecycleSink>>>,
}

impl RemoteListenerHandle {
    pub(crate) fn new() -> Self {
        Self {
            active: Arc::new(Mutex::new(None)),
            sessions: RemoteSessionRegistry::new(),
            stream: Arc::new(std::sync::OnceLock::new()),
            lifecycle: Arc::new(std::sync::OnceLock::new()),
        }
    }

    /// The process-wide live-session registry.
    pub(crate) fn sessions(&self) -> &RemoteSessionRegistry {
        &self.sessions
    }

    pub(crate) fn stream(&self) -> Option<sequencer::RemoteStreamHandle> {
        self.stream.get().cloned()
    }

    /// Publishes the stream handle. Returns `false` if one was already installed.
    pub(crate) fn install_stream(&self, stream: sequencer::RemoteStreamHandle) -> bool {
        self.stream.set(stream).is_ok()
    }

    pub(crate) fn lifecycle_sink(&self) -> Option<Arc<dyn ws::SessionLifecycleSink>> {
        self.lifecycle.get().cloned()
    }

    pub(crate) fn install_lifecycle_sink(&self, sink: Arc<dyn ws::SessionLifecycleSink>) -> bool {
        self.lifecycle.set(sink).is_ok()
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
    let base: Router = base_remote_routes().with_state(state.clone());

    // The curated `/api` remount (PR 1.5-B). Merged BEFORE the fallback and the auth stack, so
    // remounted routes sit inside the same trust-header stripping, rate limiting and bearer
    // check as every other authenticated route, and an unlisted `/api` path still lands on the
    // 404 fallback. Absent or unbuildable shared state mounts NOTHING — never a fresh AppState.
    let router = match state.remount() {
        Some(shared) => match fetch_remount::remount_router(shared) {
            Ok(build) => base.merge(build.router),
            Err(error) => {
                tracing::error!(%error, "remote fetch remount refused to build; /api stays unmounted");
                base
            }
        },
        None => {
            tracing::warn!(
                "shared :3847 AppState is not managed; remote fetch remount stays unmounted"
            );
            base
        }
    };

    router
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
}

/// The listener's own routes, before the remount merge and the auth stack.
fn base_remote_routes() -> Router<RemoteRouterState> {
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
            REVOKE_PATH,
            post(self_revoke_handler)
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
        .route(
            INVOKE_PATH,
            post(invoke::invoke_handler)
                .options(remote_preflight_handler)
                // C-16: `/invoke` carries command arguments, never payload bytes. Attachments
                // have their own endpoint with its own, larger cap.
                .layer(DefaultBodyLimit::max(REMOTE_INVOKE_BODY_LIMIT_BYTES)),
        )
        .route(
            ATTACHMENT_UPLOAD_PATH,
            post(attachment_upload_handler)
                .options(remote_preflight_handler)
                .layer(DefaultBodyLimit::max(REMOTE_ATTACHMENT_MAX_BYTES)),
        )
        .route(
            ATTACHMENT_FETCH_PATH,
            get(attachment_fetch_handler).options(remote_preflight_handler),
        )
        .route(
            ws::WS_EVENTS_PATH,
            get(ws::ws_events_handler).options(remote_preflight_handler),
        )
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

/// Everything the listener core needs out of the Tauri `AppHandle`, resolved one frame earlier.
///
/// The listener never wanted an `AppHandle` as such — it wanted three things the handle happens
/// to own: the Tauri command registry (already abstracted behind [`RemoteInvokeDispatcher`]),
/// the app-owned data dir, and the shared :3847 state registered at `server_boot`. Resolving
/// them in the thin `AppHandle` wrapper means the bind/persist/serve core can be tested without
/// constructing a Wry `AppHandle` — which panics off the main thread on macOS (`On macOS,
/// EventLoop must be created on the main thread!`) and made all of `listener_tests`
/// permanently red.
///
/// Resolution *failures* are carried, not logged, so the core still reports them at the exact
/// point in the start sequence it always did — including not at all when the listener is
/// already running and returns early.
pub(crate) struct RemoteListenerRuntime {
    invoke_dispatcher: Arc<dyn RemoteInvokeDispatcher>,
    /// `Err` carries the `AppPaths` failure text so the core logs what it always logged.
    attachment_root: Result<PathBuf, String>,
    /// `None` when `server_boot` never managed the shared state; the core keeps `/api` unmounted.
    remount: Option<Arc<fetch_remount::SharedHttpAppState>>,
}

impl RemoteListenerRuntime {
    /// Production resolution. Identical to what the core used to do inline, one frame earlier.
    pub(crate) fn from_app_handle(app_handle: &tauri::AppHandle) -> Self {
        Self {
            invoke_dispatcher: TauriRemoteInvokeDispatcher::shared(app_handle.clone()),
            attachment_root: crate::application::app_paths::AppPaths::from_app_handle(app_handle)
                .map(|paths| paths.app_data_dir)
                .map_err(|error| error.to_string()),
            remount: app_handle
                .try_state::<Arc<fetch_remount::SharedHttpAppState>>()
                .map(|shared| Arc::clone(shared.inner())),
        }
    }

    /// Test resolution: a dispatcher and nothing else, so attachments and the `/api` remount
    /// take exactly the fail-closed branches they take in production when wiring is absent.
    ///
    /// Also reachable under `test-utils` (not just `cfg(test)`) so the PR 3.2 harness — and the
    /// PR 3.4 integration binary built on it — can boot the listener without a Wry `AppHandle`.
    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn for_tests(invoke_dispatcher: Arc<dyn RemoteInvokeDispatcher>) -> Self {
        Self {
            invoke_dispatcher,
            attachment_root: Err("no app data dir in tests".to_string()),
            remount: None,
        }
    }
}

/// Binds and serves the remote listener, persisting the enablement flag once the bind succeeds.
///
/// Ordering is deliberate: refuse → bind → persist → spawn. A refused or failed bind never
/// leaves `enabled = true` behind, and a failed persist releases the socket.
pub(crate) async fn start_listener(
    app_handle: &tauri::AppHandle,
    handle: &RemoteListenerHandle,
    store: &RemoteHostSettingsStore,
    provider: &dyn TailnetSelfAddressProvider,
    tailscale: &dyn TailscaleCommandRunner,
) -> Result<SocketAddr, RemoteListenerError> {
    start_listener_with_runtime(
        RemoteListenerRuntime::from_app_handle(app_handle),
        handle,
        store,
        provider,
        tailscale,
    )
    .await
}

/// Listener core, parameterised over [`RemoteListenerRuntime`] instead of the Wry `AppHandle`.
///
/// Production keeps the `AppHandle` wrapper above, so the resolution path and its fail-closed
/// behaviour are byte-for-byte what they were.
pub(crate) async fn start_listener_with_runtime(
    runtime: RemoteListenerRuntime,
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
                degraded_kind: None,
            },
            Err(error) => {
                let degraded_reason = tailscale_serve_degraded_reason(&error);
                let degraded_kind = tailscale_serve_degraded_kind(&error);
                tracing::warn!(%error, address = %bound_address, "Tailscale Serve unavailable; remote listener remains loopback-only");
                // Serve mode means RalphX owns the :443 mapping, and this start did not
                // establish it. A mapping left by an earlier (crashed) Serve run would keep
                // forwarding tailnet :443 at whatever now holds that loopback port, while
                // this listener reports itself loopback-only. Clearing is idempotent.
                sweep_stale_serve_mapping(tailscale).await;
                RemoteServeStatus {
                    active: false,
                    degraded_reason: Some(degraded_reason),
                    degraded_kind: Some(degraded_kind),
                }
            }
        }
    } else {
        // Deliberately does NOT clear Serve state. `tailscale serve --bg` is durable in
        // tailscaled and can outlive a crashed Serve run, but the release command is a
        // blanket `serve --https=443 off`: sweeping it from a direct-exposure start would
        // delete whatever the USER mapped onto :443 for their own services. RalphX only owns
        // :443 while it is the one configuring it (the Serve arm above). Residual, accepted:
        // a crash under Serve leaves a mapping this process cannot distinguish from the
        // user's own, so it is cleared on the next Serve start/stop, not by guesswork.
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
    let auth =
        RemoteAuthContext::from_db(store.db(), handle.sessions.clone(), settings.exposure_mode);
    // One SQLite store backs both remote request idempotency and attachment metadata; it is
    // built from the SAME `DbConnection` as the auth context so a replay and the device row it
    // is scoped to can never come from different connections.
    let idempotency_store = Arc::new(SqliteRemoteRequestDedupRepository::from_db(store.db()));
    let mut state = RemoteRouterState::new_with_invoke_dispatcher(
        settings.environment_id.as_str(),
        auth,
        runtime.invoke_dispatcher,
    )
    .with_dedup(Arc::new(RemoteDedupState::new(idempotency_store.clone())))
    // Installed at app setup, not here: the listener toggle governs network exposure only, so
    // a restart of the listener must not restart the stream (P-15, P-23).
    .with_stream(handle.stream());
    // The attachment root is app-owned. If it cannot be resolved the routes stay unwired and
    // fail closed with `REMOTE_INTERNAL_ERROR` — never a fallback to a guessed directory. The
    // resolution itself happened in the `AppHandle` wrapper; the failure is reported here, at
    // the same point in the start sequence it always was.
    match runtime.attachment_root {
        Ok(app_data_dir) => {
            state = state.with_attachments(Arc::new(RemoteAttachmentContext::new(
                idempotency_store,
                &app_data_dir,
            )));
        }
        Err(error) => {
            tracing::error!(%error, "app data dir unavailable; remote attachments stay disabled");
        }
    }
    // R-8: reuse the EXACT `Arc<AppState>` :3847 serves from. If `server_boot` never managed it
    // the curated `/api` remount stays unmounted — a fresh clone here would recreate precisely
    // the invoke-vs-fetch divergence the newtype exists to eliminate.
    match runtime.remount {
        Some(shared) => {
            state = state.with_remount(shared);
        }
        None => {
            tracing::error!(
                "shared :3847 AppState is not managed; remote fetch remount stays unmounted"
            );
        }
    }
    if let Some(sink) = handle.lifecycle_sink() {
        state = state.with_lifecycle_sink(sink);
    }
    let router = remote_router(state);

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
    app_handle: &tauri::AppHandle,
    handle: &RemoteListenerHandle,
    store: &RemoteHostSettingsStore,
    provider: &dyn TailnetSelfAddressProvider,
    tailscale: &dyn TailscaleCommandRunner,
    exposure_mode: RemoteExposureMode,
) -> Result<RemoteHostSettings, RemoteListenerError> {
    apply_exposure_mode_with_runtime(
        RemoteListenerRuntime::from_app_handle(app_handle),
        handle,
        store,
        provider,
        tailscale,
        exposure_mode,
    )
    .await
}

/// See [`start_listener_with_runtime`] for why the core is parameterised over the runtime.
pub(crate) async fn apply_exposure_mode_with_runtime(
    runtime: RemoteListenerRuntime,
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

    match start_listener_with_runtime(runtime, handle, store, provider, tailscale).await {
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
    app_handle: &tauri::AppHandle,
    handle: &RemoteListenerHandle,
    store: &RemoteHostSettingsStore,
    provider: &dyn TailnetSelfAddressProvider,
    tailscale: &dyn TailscaleCommandRunner,
) -> Result<Option<SocketAddr>, RemoteListenerError> {
    auto_start_if_enabled_with_runtime(
        RemoteListenerRuntime::from_app_handle(app_handle),
        handle,
        store,
        provider,
        tailscale,
    )
    .await
}

/// See [`start_listener_with_runtime`] for why the core is parameterised over the runtime.
pub(crate) async fn auto_start_if_enabled_with_runtime(
    runtime: RemoteListenerRuntime,
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
    start_listener_with_runtime(runtime, handle, store, provider, tailscale)
        .await
        .map(Some)
}

/// Closes `remote_sessions` rows that a previous process left open.
///
/// The graceful paths close their own rows (`SessionGuard` on socket close, `stop_listener` on
/// disable). A crash, force quit, or power loss closes nothing, and every one of those rows keeps
/// showing up in the pane's "connections currently open against this Mac" list forever — the owner
/// can only clear them one disconnect at a time. Startup is the one moment where the classification
/// is unambiguous: no socket has been admitted yet in this process, so every open row belongs to a
/// dead one. Live completion and startup recovery therefore terminalise the same way.
pub(crate) async fn close_orphaned_remote_sessions_from_handle(app_handle: &tauri::AppHandle) {
    let Some(state) = app_handle.try_state::<crate::AppState>() else {
        tracing::warn!("AppState is unavailable; skipping remote session reconciliation");
        return;
    };
    let store = RemoteHostSettingsStore::from_db(state.db.clone());
    // Same `get()` gate as capture installation: an unconfigured host has no sessions to reconcile,
    // and a read must never mint the row that defines "configured".
    match store.get().await {
        Ok(Some(_)) => {}
        Ok(None) => return,
        Err(error) => {
            tracing::error!(
                %error,
                "Reading remote host settings failed; skipping remote session reconciliation"
            );
            return;
        }
    }

    let sessions = SqliteRemoteAccessRepository::from_db(state.db.clone());
    match sessions
        .close_all(&crate::remote_server::auth::now_timestamp())
        .await
    {
        Ok(0) => {}
        Ok(closed) => tracing::info!(
            closed,
            "Closed remote session rows left open by a previous process"
        ),
        Err(error) => {
            tracing::error!(%error, "Closing remote session rows from a previous process failed")
        }
    }
}

/// Installs the capture bank + durable sequencer + pruner when host mode is **configured**.
///
/// P-23: this runs at app setup and is deliberately independent of listener enablement. The
/// listener toggle governs network exposure only, so events emitted while the listener is off are
/// still captured, sequenced, and replayable after a re-enable — a reconnect never warm-resumes
/// across an unrecorded gap (§5.2, §3.4 capture-at-setup).
///
/// "Configured" means the `remote_host_settings` row exists. It is read with `get()`, never
/// `get_or_create()`: minting the row here would install capture on every host that merely
/// launched the app, which §3.4 explicitly rules out ("if host mode was never configured, capture
/// is not installed — no cost").
pub(crate) async fn install_remote_stream_from_handle(app_handle: &tauri::AppHandle) {
    let Some(state) = app_handle.try_state::<crate::AppState>() else {
        tracing::warn!("AppState is unavailable; skipping remote capture installation");
        return;
    };
    let store = RemoteHostSettingsStore::from_db(state.db.clone());
    match store.get().await {
        Ok(Some(_)) => {}
        Ok(None) => {
            tracing::debug!("Remote host mode is not configured; capture stays uninstalled");
            return;
        }
        Err(error) => {
            // Fail closed and loudly: silently skipping capture would produce a host that looks
            // healthy but records nothing, and the gap would only surface as a client reset.
            tracing::error!(%error, "Reading remote host settings failed; capture stays uninstalled");
            return;
        }
    }

    let handle = remote_listener_handle(app_handle);
    if handle.stream().is_some() {
        tracing::debug!("Remote durable sequencer is already installed");
        return;
    }

    let (feed, receivers) = capture::CaptureFeed::channels(REMOTE_CAPTURE_QUEUE_CAPACITY);
    let log = Arc::new(SqliteRemoteEventLogRepository::from_db(state.db.clone()));
    let leases = RetentionLeaseRegistry::new();
    let stream = match sequencer::RemoteSequencer::start(log, receivers, leases).await {
        Ok(stream) => stream,
        Err(error) => {
            tracing::error!(%error, "Remote durable sequencer failed to start; capture stays uninstalled");
            return;
        }
    };

    // Capture is installed only after the sequencer is live, so no captured event can be dropped
    // into a channel nobody is draining yet.
    capture::RemoteEventCapture::install(app_handle.clone(), feed);
    if !handle.install_stream(stream.clone()) {
        tracing::warn!("A remote durable sequencer was already installed; keeping the first");
        return;
    }
    handle.install_lifecycle_sink(Arc::new(ws::TauriLifecycleSink(app_handle.clone())));
    retention::spawn_pruner(stream);
    tracing::info!("Remote event capture and durable sequencer installed at app setup");
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
        app_handle,
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

fn tailscale_serve_degraded_kind(error: &TailscaleServeError) -> RemoteServeDegradedKind {
    match error {
        TailscaleServeError::CliUnavailable => RemoteServeDegradedKind::CliUnavailable,
        TailscaleServeError::Launch(_) => RemoteServeDegradedKind::LaunchFailed,
        TailscaleServeError::Timeout => RemoteServeDegradedKind::Timeout,
        TailscaleServeError::Output(_) | TailscaleServeError::Exit(_) => {
            RemoteServeDegradedKind::CommandFailed
        }
    }
}

/// Best-effort clearing of a Serve mapping this process does not own.
///
/// Separate from [`release_serve_best_effort`] because the common outcome here is "no
/// Tailscale installed", which is not worth a warning on every loopback start.
async fn sweep_stale_serve_mapping(tailscale: &dyn TailscaleCommandRunner) {
    if let Err(error) = tailscale.run_serve_release().await {
        tracing::debug!(%error, "No stale Tailscale Serve mapping was cleared");
    }
}

/// Best-effort Serve release on app exit.
///
/// `stop_listener` and `apply_exposure_mode` are the only other release paths, so a clean
/// quit used to leave the tailnet `:443 → 127.0.0.1:<port>` forward pointing at a loopback
/// port this process no longer owns — the same residue a crash leaves, mitigated only by the
/// next serve start's sweep (§5.3 item 2).
///
/// Bounded and non-propagating by construction: exit must not be blocked by a CLI that hangs.
pub(crate) fn release_serve_mapping_on_exit(handle: &RemoteListenerHandle) {
    let handle = handle.clone();
    let released = tauri::async_runtime::block_on(async move {
        release_serve_mapping_for_exit(&handle, &RealTailscaleCommandRunner).await
    });
    if released {
        tracing::info!("Released the Tailscale Serve mapping on exit");
    }
}

/// The injectable core of [`release_serve_mapping_on_exit`]. Returns whether a release was
/// both warranted and completed inside the budget.
pub(crate) async fn release_serve_mapping_for_exit(
    handle: &RemoteListenerHandle,
    tailscale: &dyn TailscaleCommandRunner,
) -> bool {
    let Some(bind_address) = handle.bound_address().await else {
        return false;
    };
    if !handle.serve_status().await.active {
        return false;
    }
    tokio::time::timeout(
        SERVE_EXIT_RELEASE_TIMEOUT,
        release_serve_best_effort(tailscale, bind_address),
    )
    .await
    .is_ok()
}

/// Exit is not the place to wait on a hung CLI; the next serve start still sweeps.
const SERVE_EXIT_RELEASE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

async fn release_serve_best_effort(
    tailscale: &dyn TailscaleCommandRunner,
    bind_address: SocketAddr,
) {
    if let Err(error) = tailscale.run_serve_release().await {
        tracing::warn!(%error, address = %bind_address, "Tailscale Serve release failed");
    }
}
