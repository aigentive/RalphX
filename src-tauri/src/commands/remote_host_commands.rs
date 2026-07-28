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
    /// The CONFIGURED port, not necessarily the bound one: `RALPHX_REMOTE_PORT` overrides the
    /// bind (`effective_remote_port`, `remote_server/settings.rs`) and that override surfaces
    /// only through `bind_address`. PR 1.7 must derive advertised URLs from `bind_address`, not
    /// from this field, or a dev-parity host will advertise a port nothing listens on.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_server::settings::DEFAULT_REMOTE_PORT;

    #[tokio::test]
    async fn listener_status_reports_not_running_for_a_fresh_handle() {
        let handle = RemoteListenerHandle::new();
        let settings = RemoteHostSettings {
            enabled: false,
            exposure_mode: RemoteExposureMode::Serve,
            port: DEFAULT_REMOTE_PORT,
            environment_id: "env-1".to_string(),
        };

        let status = listener_status(settings, &handle).await;

        assert!(!status.enabled);
        assert_eq!(status.exposure_mode, RemoteExposureMode::Serve);
        assert_eq!(status.port, DEFAULT_REMOTE_PORT);
        assert_eq!(status.environment_id, "env-1");
        assert!(!status.running);
        assert!(status.bind_address.is_none());
        assert!(!status.serve_active);
        assert!(status.serve_degraded_reason.is_none());
    }
}
