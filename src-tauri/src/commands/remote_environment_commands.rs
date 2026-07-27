//! Client-side Tauri commands for the remote environment registry and the
//! Rust-proxy surface (§6.1, §6.4).
//!
//! P-18 by construction: no command in this module returns a device token or any
//! secret material, and there is no credential-fetch command. Responses go through
//! `RemoteEnvironmentSummary`, which never carries the token. The active-environment
//! id used by proxy authorization comes from the Rust-side mirror, never from a
//! trusted JS argument (P-26) — the `id` args below only SELECT a target, the
//! service decides whether that target is authorized.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::remote_environment_service::{
    RemoteEnvironmentError, RemoteEnvironmentService,
};
use crate::domain::entities::remote_environment::{RemoteEnvironment, RemoteEnvironmentStatus};
use crate::AppState;

/// JS-facing projection of a paired remote environment.
///
/// Explicit field allowlist: the device token and the Keychain reference are
/// deliberately NOT part of this struct (P-18).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEnvironmentSummary {
    pub id: String,
    pub environment_id: String,
    pub name: String,
    pub base_url: String,
    pub candidate_urls: Vec<String>,
    pub scopes: Vec<ralphx_remote_protocol::Scope>,
    pub protocol_version: u32,
    pub status: RemoteEnvironmentStatus,
    pub created_at: String,
    pub last_connected_at: Option<String>,
}

impl From<RemoteEnvironment> for RemoteEnvironmentSummary {
    fn from(env: RemoteEnvironment) -> Self {
        Self {
            id: env.id.as_str().to_string(),
            environment_id: env.environment_id,
            name: env.name,
            base_url: env.base_url,
            candidate_urls: env.candidate_urls,
            scopes: env.scopes,
            protocol_version: env.protocol_version,
            status: env.status,
            created_at: env.created_at,
            last_connected_at: env.last_connected_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairRemoteEnvironmentInput {
    pub url: String,
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEnvironmentIdInput {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInvokeInput {
    pub id: String,
    pub request_id: String,
    pub cmd: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

/// Fetch seam as of PR 2.1: id + path only.
///
/// PR 2.2 migrates `backendFetch(path, init)` call sites that POST JSON bodies and branch on
/// `res.ok`/`res.status`, so it MUST widen this additively — `method`, `body`, `headers` here,
/// and a `{ status, body }` envelope out of `remote_fetch` — rather than reinterpreting the
/// current bare-`Value` return. Nothing consumes the command yet, so the widening is not a
/// break; silently keeping this shape is.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFetchInput {
    pub id: String,
    pub path: String,
}

fn service<'a>(state: &'a State<'_, AppState>) -> &'a RemoteEnvironmentService {
    &state.remote_environment_service
}

fn to_command_error(error: RemoteEnvironmentError) -> String {
    error.to_command_error()
}

/// Performs the pairing exchange in the Rust backend (§4.2): descriptor →
/// pair → row (`pending_add`) → Keychain → `active`. The token goes straight
/// to the Keychain and is absent from the response.
#[tauri::command]
pub async fn pair_remote_environment(
    input: PairRemoteEnvironmentInput,
    state: State<'_, AppState>,
) -> Result<RemoteEnvironmentSummary, String> {
    service(&state)
        .pair(&input.url, &input.code, &input.name)
        .await
        .map(RemoteEnvironmentSummary::from)
        .map_err(to_command_error)
}

#[tauri::command]
pub async fn list_remote_environments(
    state: State<'_, AppState>,
) -> Result<Vec<RemoteEnvironmentSummary>, String> {
    service(&state)
        .list()
        .await
        .map(|environments| {
            environments
                .into_iter()
                .map(RemoteEnvironmentSummary::from)
                .collect()
        })
        .map_err(to_command_error)
}

/// Staged removal: mark `pending_delete` → best-effort host revoke → Keychain
/// delete → row delete (P-27).
#[tauri::command]
pub async fn remove_remote_environment(
    input: RemoteEnvironmentIdInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    service(&state)
        .remove(&input.id)
        .await
        .map_err(to_command_error)
}

/// Reads the Rust-side authoritative active environment id.
#[tauri::command]
pub async fn get_active_environment(state: State<'_, AppState>) -> Result<String, String> {
    Ok(service(&state).active_environment_id().await)
}

/// Switches the Rust-side authoritative active environment (§6.4). The webview's
/// `environmentStore` mirrors this value; proxy authorization reads only the
/// Rust copy.
#[tauri::command]
pub async fn set_active_environment(
    input: RemoteEnvironmentIdInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    service(&state)
        .set_active_environment(&input.id)
        .await
        .map_err(to_command_error)
}

/// Opens the Rust-owned outbound connection for an environment (WS body lands in
/// PR 2.3 — until then this returns `NOT_CONNECTED` after authorization).
#[tauri::command]
pub async fn remote_connect(
    input: RemoteEnvironmentIdInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    service(&state)
        .connect(&input.id)
        .await
        .map_err(to_command_error)
}

#[tauri::command]
pub async fn remote_disconnect(
    input: RemoteEnvironmentIdInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    service(&state)
        .disconnect(&input.id)
        .await
        .map_err(to_command_error)
}

/// Forwards one command invoke through the Rust proxy. Active-env-bound (P-26);
/// the bearer stays in Rust. HTTP transport lands in PR 2.2.
#[tauri::command]
pub async fn remote_invoke(
    input: RemoteInvokeInput,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    service(&state)
        .invoke(&input.id, &input.request_id, &input.cmd, input.args)
        .await
        .map_err(to_command_error)
}

/// Fetches a host resource through the Rust proxy. Health paths (descriptor,
/// health probe) are allowed for background environments; everything else is
/// active-env-bound (P-26).
#[tauri::command]
pub async fn remote_fetch(
    input: RemoteFetchInput,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    service(&state)
        .fetch(&input.id, &input.path)
        .await
        .map_err(to_command_error)
}

#[cfg(test)]
#[path = "remote_environment_commands_tests.rs"]
mod tests;
