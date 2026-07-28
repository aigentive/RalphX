//! Unauthenticated discovery surface for the remote listener.
//!
//! The environment descriptor is deliberately minimal: it is the one pre-auth response a
//! stranger can read, so it publishes identity and version negotiation data only (§3.1, §4.6).

use std::net::Ipv4Addr;
use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use ralphx_remote_protocol::{EnvironmentDescriptor, PROTOCOL_VERSION};
use serde::Serialize;

use crate::remote_server::attachments::RemoteAttachmentContext;
use crate::remote_server::auth::RemoteAuthContext;
use crate::remote_server::dedup::RemoteDedupState;
use crate::remote_server::fetch_remount::SharedHttpAppState;
use crate::remote_server::invoke::RemoteInvokeDispatcher;
use crate::remote_server::sequencer::RemoteStreamHandle;
use crate::remote_server::settings::RemoteExposureMode;
use crate::remote_server::ws::{NoopLifecycleSink, SessionLifecycleSink};

/// Oldest client protocol this host will negotiate with.
///
/// Host acceptance policy, not protocol shape — it lives here rather than in the protocol
/// crate so a host can tighten it without a protocol revision.
pub(crate) const MIN_CLIENT_PROTOCOL: u32 = PROTOCOL_VERSION;

/// Shared state for the remote router.
///
/// The auth context is **not** optional: there is no router shape that can serve a
/// non-allowlisted route without a device store to check against (A-2).
#[derive(Clone)]
pub(crate) struct RemoteRouterState {
    environment_id: Arc<str>,
    auth: Arc<RemoteAuthContext>,
    invoke_dispatcher: Arc<dyn RemoteInvokeDispatcher>,
    /// The durable stream, installed at app setup when host mode is configured (P-23).
    ///
    /// `Option` because the listener and the stream have independent lifetimes by design: the
    /// listener toggle governs network exposure only. A router without a stream still serves every
    /// HTTP route and refuses only the WS upgrade, explicitly.
    stream: Option<RemoteStreamHandle>,
    lifecycle: Arc<dyn SessionLifecycleSink>,
    /// Request idempotency (PR 1.5-C).
    ///
    /// `Option` because the auth-only router shapes used by several 1.2/1.3 tests build a state
    /// with no dedup store. Absence is fail-CLOSED for mutating commands: [`invoke_handler`]
    /// refuses them with `REMOTE_INTERNAL_ERROR` rather than dispatching un-deduplicated, so a
    /// wiring regression can never silently restore at-least-once semantics.
    ///
    /// [`invoke_handler`]: crate::remote_server::invoke::invoke_handler
    dedup: Option<Arc<RemoteDedupState>>,
    /// Attachment store + app-owned storage root (PR 1.5-C).
    ///
    /// `Option` for the same fail-closed reason as `dedup`: if the app data dir could not be
    /// resolved the attachment handlers refuse rather than write to a guessed directory.
    attachments: Option<Arc<RemoteAttachmentContext>>,
    /// The shared :3847 `AppState` behind the curated fetch remount (PR 1.5-B, R-8).
    ///
    /// `Option` for the same fail-closed reason as `dedup`: when the shared state was never
    /// managed, the `/api` routes are never mounted at all — they answer with the 404 fallback
    /// exactly like any other unlisted path. The remount NEVER falls back to a fresh
    /// `AppState`, because a second graph would reintroduce the invoke-vs-fetch divergence
    /// this field exists to eliminate.
    remount: Option<Arc<SharedHttpAppState>>,
}

impl RemoteRouterState {
    /// The `AppHandle`-taking `new` is gone: `start_listener` — its only caller — now resolves
    /// every `AppHandle`-derived input (dispatcher, attachment root, shared remount state) into
    /// a `RemoteListenerRuntime` one frame earlier, so the listener core can be tested without a
    /// Wry handle. Production still lands on `TauriRemoteInvokeDispatcher::shared(app_handle)`,
    /// `AppPaths::from_app_handle` and `try_state::<Arc<SharedHttpAppState>>()` exactly as before.
    pub(crate) fn new_with_invoke_dispatcher(
        environment_id: impl Into<Arc<str>>,
        auth: RemoteAuthContext,
        invoke_dispatcher: Arc<dyn RemoteInvokeDispatcher>,
    ) -> Self {
        Self {
            environment_id: environment_id.into(),
            auth: Arc::new(auth),
            invoke_dispatcher,
            stream: None,
            lifecycle: Arc::new(NoopLifecycleSink),
            dedup: None,
            attachments: None,
            remount: None,
        }
    }

    pub(crate) fn with_remount(mut self, remount: Arc<SharedHttpAppState>) -> Self {
        self.remount = Some(remount);
        self
    }

    /// The shared :3847 state, or `None` when it was never managed.
    ///
    /// Callers must treat `None` as "mount nothing", never as "build a fresh state".
    pub(crate) fn remount(&self) -> Option<&Arc<SharedHttpAppState>> {
        self.remount.as_ref()
    }

    pub(crate) fn with_dedup(mut self, dedup: Arc<RemoteDedupState>) -> Self {
        self.dedup = Some(dedup);
        self
    }

    pub(crate) fn with_attachments(mut self, attachments: Arc<RemoteAttachmentContext>) -> Self {
        self.attachments = Some(attachments);
        self
    }

    pub(crate) fn with_stream(mut self, stream: Option<RemoteStreamHandle>) -> Self {
        self.stream = stream;
        self
    }

    pub(crate) fn with_lifecycle_sink(mut self, lifecycle: Arc<dyn SessionLifecycleSink>) -> Self {
        self.lifecycle = lifecycle;
        self
    }

    pub(crate) fn environment_id(&self) -> &str {
        &self.environment_id
    }

    pub(crate) fn auth(&self) -> &RemoteAuthContext {
        &self.auth
    }

    pub(crate) fn invoke_dispatcher(&self) -> Arc<dyn RemoteInvokeDispatcher> {
        Arc::clone(&self.invoke_dispatcher)
    }

    pub(crate) fn stream(&self) -> Option<&RemoteStreamHandle> {
        self.stream.as_ref()
    }

    /// The dedup state, or `None` when the router was built without one.
    ///
    /// Callers must treat `None` as a refusal for mutating commands, never as "skip dedup".
    pub(crate) fn dedup(&self) -> Option<&Arc<RemoteDedupState>> {
        self.dedup.as_ref()
    }

    /// The attachment context, or `None` when storage could not be wired.
    pub(crate) fn attachments(&self) -> Option<&Arc<RemoteAttachmentContext>> {
        self.attachments.as_ref()
    }

    pub(crate) fn lifecycle(&self) -> Arc<dyn SessionLifecycleSink> {
        Arc::clone(&self.lifecycle)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteHealthBody {
    pub status: &'static str,
}

// Consumed by PR 1.7's Remote Access pane (endpoint list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AdvertisedEndpointKind {
    LoopbackServe,
    TailnetDirect,
}

// Consumed by PR 1.7's Remote Access pane (endpoint list).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvertisedEndpoint {
    pub kind: AdvertisedEndpointKind,
    pub url: String,
    pub available: bool,
}

/// Describes remote URLs from already-observed reachability facts without granting access.
///
/// Scheme per mode is a transport fact, not a preference: only Serve terminates TLS at the
/// tailnet edge, so it advertises `https://<magicdns>`. The listener itself is plain-HTTP axum
/// with no TLS acceptor, and §4.4 assigns direct-mode confidentiality to WireGuard, not to app
/// TLS ("the Rust backend terminates plain HTTP"), so direct exposure advertises `http://` —
/// an `https://` direct URL would always fail its handshake against the plaintext socket.
///
/// `port` is the port the listener is actually bound on; callers must pass the bound port, not
/// the persisted setting, because `RALPHX_REMOTE_PORT` can override it.
// Consumed by PR 1.7's Remote Access pane (endpoint list).
pub(crate) fn advertised_endpoints(
    exposure_mode: RemoteExposureMode,
    port: u16,
    magicdns_name: Option<&str>,
    serve_reachable: bool,
    tailnet_self_ip: Option<Ipv4Addr>,
) -> Vec<AdvertisedEndpoint> {
    match exposure_mode {
        // Defensive re-normalization: `TailscaleStatus::magicdns_name()` already trims the
        // trailing dot, but this function also takes names from callers/settings.
        RemoteExposureMode::Serve => magicdns_name
            .map(str::trim)
            .map(|name| name.trim_end_matches('.'))
            .filter(|name| !name.is_empty())
            .map(|name| AdvertisedEndpoint {
                kind: AdvertisedEndpointKind::LoopbackServe,
                url: format!("https://{name}"),
                available: serve_reachable,
            })
            .into_iter()
            .collect(),
        RemoteExposureMode::TailnetDirect => tailnet_self_ip
            .map(|address| AdvertisedEndpoint {
                kind: AdvertisedEndpointKind::TailnetDirect,
                url: format!("http://{address}:{port}"),
                available: true,
            })
            .into_iter()
            .collect(),
    }
}

/// Builds the five-field descriptor published at `/.well-known/ralphx/environment`.
pub(crate) fn environment_descriptor(environment_id: &str) -> EnvironmentDescriptor {
    EnvironmentDescriptor {
        environment_id: environment_id.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        protocol_version: PROTOCOL_VERSION,
        min_client_protocol: MIN_CLIENT_PROTOCOL,
        platform: std::env::consts::OS.to_string(),
    }
}

pub(crate) async fn environment_descriptor_handler(
    State(state): State<RemoteRouterState>,
) -> Json<EnvironmentDescriptor> {
    Json(environment_descriptor(state.environment_id()))
}

pub(crate) async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, Json(RemoteHealthBody { status: "ok" }))
}
