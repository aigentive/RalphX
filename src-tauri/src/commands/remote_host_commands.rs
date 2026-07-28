//! Host-local Tauri commands for the remote listener (§5.2).
//!
//! These are loopback-only by construction: no equivalent route is mounted on :3849 (§3.1).

use std::net::{IpAddr, SocketAddr};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::infrastructure::tailscale::{
    parse_status, RealTailscaleCommandRunner, TailscaleCommandRunner, TailscaleSelfAddressProvider,
};
use crate::remote_server::endpoints::{advertised_endpoints, AdvertisedEndpoint};
use crate::remote_server::settings::{
    RemoteExposureMode, RemoteHostSettings, RemoteHostSettingsStore, DEFAULT_REMOTE_PORT,
};
use crate::remote_server::{
    apply_exposure_mode, remote_listener_handle, start_listener, stop_listener,
    RemoteListenerHandle, RemoteServeDegradedKind,
};
use crate::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteListenerStatus {
    pub enabled: bool,
    pub exposure_mode: RemoteExposureMode,
    /// The CONFIGURED port, not necessarily the bound one: `RALPHX_REMOTE_PORT` overrides the
    /// bind (`effective_remote_port`, `remote_server/settings.rs`) and that override surfaces
    /// only through `bind_address`. PR 1.7 must derive advertised URLs from `bind_address`, not
    /// from this field, or a dev-parity host will advertise a port nothing listens on.
    pub port: u16,
    pub environment_id: String,
    /// Whether this process holds a bound listener. Like `serve_active`, a start-time fact:
    /// it is not a liveness probe of the socket or of `tailscaled`.
    pub running: bool,
    pub bind_address: Option<String>,
    /// Serve state as of the last start (see `RemoteServeStatus`) — never re-probed.
    pub serve_active: bool,
    pub serve_degraded_reason: Option<String>,
    /// Machine-readable degradation cause; branch on this, never on the reason prose
    /// (rule 5).
    pub serve_degraded_kind: Option<RemoteServeDegradedKind>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetRemoteExposureModeInput {
    pub exposure_mode: RemoteExposureMode,
}

fn settings_store(state: &State<'_, AppState>) -> RemoteHostSettingsStore {
    RemoteHostSettingsStore::from_db(state.db.clone())
}

async fn listener_status(
    settings: RemoteHostSettings,
    handle: &RemoteListenerHandle,
) -> RemoteListenerStatus {
    let bind_address = handle.bound_address().await;
    let serve = handle.serve_status().await;
    RemoteListenerStatus {
        enabled: settings.enabled,
        exposure_mode: settings.exposure_mode,
        port: settings.port,
        environment_id: settings.environment_id,
        running: bind_address.is_some(),
        bind_address: bind_address.map(|address| address.to_string()),
        serve_active: serve.active,
        serve_degraded_reason: serve.degraded_reason,
        serve_degraded_kind: serve.degraded_kind,
    }
}

async fn advertised_endpoints_for_status(
    exposure_mode: RemoteExposureMode,
    bound_address: Option<SocketAddr>,
    serve_reachable: bool,
    tailscale: &dyn TailscaleCommandRunner,
) -> Vec<AdvertisedEndpoint> {
    let Some(bound_address) = bound_address else {
        return Vec::new();
    };
    let status = match tailscale.run_status().await {
        Ok(stdout) => parse_status(&stdout).ok(),
        Err(_) => None,
    };
    let magicdns_name = status
        .as_ref()
        .and_then(|status| status.magicdns_name())
        .map(str::to_string);
    let tailnet_self_ip = status.as_ref().and_then(|status| {
        status
            .self_addresses()
            .into_iter()
            .next()
            .and_then(|address| match address {
                IpAddr::V4(address) => Some(address),
                IpAddr::V6(_) => None,
            })
    });

    advertised_endpoints(
        exposure_mode,
        bound_address.port(),
        magicdns_name.as_deref(),
        serve_reachable,
        tailnet_self_ip,
    )
}

/// The status a host that has never configured remote access reports.
///
/// Synthesised rather than minted: "configured" IS the presence of the settings row (§3.4), and a
/// configured host installs capture + the durable sequencer + the pruner on every subsequent boot.
/// Merely reading the status — which the Remote Access pane does on mount — must not buy that.
fn unconfigured_status() -> RemoteListenerStatus {
    RemoteListenerStatus {
        enabled: false,
        exposure_mode: RemoteExposureMode::Serve,
        port: DEFAULT_REMOTE_PORT,
        environment_id: String::new(),
        running: false,
        bind_address: None,
        serve_active: false,
        serve_degraded_reason: None,
        serve_degraded_kind: None,
    }
}

/// Enables remote host mode and binds the listener for the persisted exposure mode.
#[tauri::command]
pub async fn start_remote_listener(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<RemoteListenerStatus, String> {
    let store = settings_store(&state);
    let handle = remote_listener_handle(&app);
    start_listener(
        &handle,
        &store,
        &TailscaleSelfAddressProvider,
        &RealTailscaleCommandRunner,
    )
    .await
    .map_err(|error| error.to_string())?;
    // P-23: app setup installs capture + the sequencer only when the settings row ALREADY exists,
    // so a first-ever enable would otherwise run the whole process with no stream — every WS
    // subscribe answering 503 and nothing captured — until the app restarted. `start_listener`
    // has just minted the row, and the install is idempotent (OnceLock), so this is the seam that
    // makes "host mode configured ⇒ capture records" true in the configuring process too.
    crate::remote_server::install_remote_stream_from_handle(&app).await;
    let settings = store.get_or_create().await.map_err(|e| e.to_string())?;
    Ok(listener_status(settings, &handle).await)
}

/// Disables remote host mode and releases the port.
#[tauri::command]
pub async fn stop_remote_listener(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<RemoteListenerStatus, String> {
    let store = settings_store(&state);
    let handle = remote_listener_handle(&app);
    stop_listener(&handle, &store, &RealTailscaleCommandRunner)
        .await
        .map_err(|error| error.to_string())?;
    let settings = store.get_or_create().await.map_err(|e| e.to_string())?;
    Ok(listener_status(settings, &handle).await)
}

/// Persists the exposure mode, restarting a running listener under the new bind policy.
#[tauri::command]
pub async fn set_remote_exposure_mode(
    input: SetRemoteExposureModeInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<RemoteListenerStatus, String> {
    let store = settings_store(&state);
    let handle = remote_listener_handle(&app);
    let settings = apply_exposure_mode(
        &handle,
        &store,
        &TailscaleSelfAddressProvider,
        &RealTailscaleCommandRunner,
        input.exposure_mode,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(listener_status(settings, &handle).await)
}

/// Reads the current listener status without changing it — including without CONFIGURING it.
#[tauri::command]
pub async fn get_remote_listener_status(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<RemoteListenerStatus, String> {
    let store = settings_store(&state);
    let handle = remote_listener_handle(&app);
    let Some(settings) = store.get().await.map_err(|e| e.to_string())? else {
        return Ok(unconfigured_status());
    };
    Ok(listener_status(settings, &handle).await)
}

/// Lists the URLs currently advertised for the running remote listener.
#[tauri::command]
pub async fn list_remote_advertised_endpoints(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Vec<AdvertisedEndpoint>, String> {
    let store = settings_store(&state);
    let handle = remote_listener_handle(&app);
    // `get()`, not `get_or_create()`: a read from the pane must not configure host mode. An
    // unconfigured host has no listener either, so the empty answer is the same one it gives when
    // the row exists and nothing is bound.
    let Some(settings) = store.get().await.map_err(|e| e.to_string())? else {
        return Ok(Vec::new());
    };
    let bound_address = handle.bound_address().await;
    if bound_address.is_none() {
        return Ok(Vec::new());
    }
    let serve_reachable = handle.serve_status().await.active;
    Ok(advertised_endpoints_for_status(
        settings.exposure_mode,
        bound_address,
        serve_reachable,
        &RealTailscaleCommandRunner,
    )
    .await)
}

#[cfg(test)]
#[path = "remote_host_commands_tests.rs"]
mod tests;
