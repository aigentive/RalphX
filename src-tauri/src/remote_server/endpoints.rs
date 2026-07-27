//! Unauthenticated discovery surface for the remote listener.
//!
//! The environment descriptor is deliberately minimal: it is the one pre-auth response a
//! stranger can read, so it publishes identity and version negotiation data only (§3.1, §4.6).

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use ralphx_remote_protocol::{EnvironmentDescriptor, PROTOCOL_VERSION};
use serde::Serialize;

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
