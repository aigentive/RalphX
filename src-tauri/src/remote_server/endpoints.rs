//! Unauthenticated discovery surface for the remote listener.
//!
//! The environment descriptor is deliberately minimal: it is the one pre-auth response a
//! stranger can read, so it publishes identity and version negotiation data only (§3.1, §4.6).

use std::sync::Arc;
use std::{net::Ipv4Addr, string::ToString};

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use ralphx_remote_protocol::{EnvironmentDescriptor, PROTOCOL_VERSION};
use serde::Serialize;

use crate::remote_server::settings::RemoteExposureMode;

/// Oldest client protocol this host will negotiate with.
///
/// Host acceptance policy, not protocol shape — it lives here rather than in the protocol
/// crate so a host can tighten it without a protocol revision.
pub(crate) const MIN_CLIENT_PROTOCOL: u32 = PROTOCOL_VERSION;

/// Shared state for the remote router.
#[derive(Clone)]
pub(crate) struct RemoteRouterState {
    environment_id: Arc<str>,
}

impl RemoteRouterState {
    pub(crate) fn new(environment_id: impl Into<Arc<str>>) -> Self {
        Self {
            environment_id: environment_id.into(),
        }
    }

    pub(crate) fn environment_id(&self) -> &str {
        &self.environment_id
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteHealthBody {
    pub status: &'static str,
}

// Consumed by PR 1.7's Remote Access pane (endpoint list).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AdvertisedEndpointKind {
    LoopbackServe,
    TailnetDirect,
}

// Consumed by PR 1.7's Remote Access pane (endpoint list).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdvertisedEndpoint {
    pub kind: AdvertisedEndpointKind,
    pub url: String,
    pub available: bool,
}

/// Describes remote URLs from already-observed reachability facts without granting access.
// Consumed by PR 1.7's Remote Access pane (endpoint list).
#[allow(dead_code)]
pub(crate) fn advertised_endpoints(
    exposure_mode: RemoteExposureMode,
    port: u16,
    magicdns_name: Option<&str>,
    serve_reachable: bool,
    tailnet_self_ip: Option<Ipv4Addr>,
) -> Vec<AdvertisedEndpoint> {
    match exposure_mode {
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
                url: format!("https://{address}:{port}"),
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
