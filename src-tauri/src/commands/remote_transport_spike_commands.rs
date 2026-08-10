//! Debug-only commands for the PR 0.3 remote transport CORS probe.

use serde::Deserialize;

use crate::remote_server::transport_spike::{
    run_desktop_proxy_stub, start_cors_probe_listener, stop_cors_probe_listener,
    DebugCorsProbeEndpoint, DebugCorsProbeOrdering, DebugDesktopProxyStubResult,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugStartRemoteTransportCorsProbeInput {
    pub ordering: DebugCorsProbeOrdering,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugDesktopProxyStubInput {
    pub ordering: DebugCorsProbeOrdering,
}

/// Starts a loopback-only fixture for the documented direct-browser CORS ordering probe.
#[tauri::command]
pub async fn debug_start_remote_transport_cors_probe(
    input: DebugStartRemoteTransportCorsProbeInput,
) -> Result<DebugCorsProbeEndpoint, String> {
    start_cors_probe_listener(input.ordering).await
}

/// Stops the loopback-only fixture started by `debug_start_remote_transport_cors_probe`.
#[tauri::command]
pub fn debug_stop_remote_transport_cors_probe() -> Result<bool, String> {
    stop_cors_probe_listener()
}

/// Runs a fixed local-IPC-to-Rust-to-loopback desktop transport probe.
#[tauri::command]
pub async fn debug_run_desktop_proxy_stub(
    input: DebugDesktopProxyStubInput,
) -> Result<DebugDesktopProxyStubResult, String> {
    run_desktop_proxy_stub(input.ordering).await
}
