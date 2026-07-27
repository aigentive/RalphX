//! Host-local Tauri commands for the remote listener (§5.2).
//!
//! These are loopback-only by construction: no equivalent route is mounted on :3849 (§3.1).

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::infrastructure::tailscale::{RealTailscaleCommandRunner, TailscaleSelfAddressProvider};
use crate::remote_server::settings::{
    RemoteExposureMode, RemoteHostSettings, RemoteHostSettingsStore,
};
use crate::remote_server::{
    apply_exposure_mode, remote_listener_handle, start_listener, stop_listener,
    RemoteListenerHandle,
};
use crate::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteListenerStatus {
    pub enabled: bool,
    pub exposure_mode: RemoteExposureMode,
    pub port: u16,
    pub environment_id: String,
    pub running: bool,
    pub bind_address: Option<String>,
    pub serve_active: bool,
    pub serve_degraded_reason: Option<String>,
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

/// Reads the current listener status without changing it.
#[tauri::command]
pub async fn get_remote_listener_status(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<RemoteListenerStatus, String> {
    let store = settings_store(&state);
    let handle = remote_listener_handle(&app);
    let settings = store.get_or_create().await.map_err(|e| e.to_string())?;
    Ok(listener_status(settings, &handle).await)
}
