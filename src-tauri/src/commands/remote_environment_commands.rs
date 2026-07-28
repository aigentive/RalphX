//! Client-side Tauri commands for the remote environment registry and the
//! Rust-proxy surface (§6.1, §6.4).
//!
//! P-18 by construction: no command in this module returns a device token or any
//! secret material, and there is no credential-fetch command. Responses go through
//! `RemoteEnvironmentSummary`, which never carries the token. The active-environment
//! id used by proxy authorization comes from the Rust-side mirror, never from a
//! trusted JS argument (P-26) — the `id` args below only SELECT a target, the
//! service decides whether that target is authorized.

use ralphx_remote_protocol::ClientFrame;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::remote_environment_service::{
    RemoteEnvironmentError, RemoteEnvironmentPreview, RemoteEnvironmentService, RemoteFetchCall,
    RemoteFetchOutcome, RemoteInvokeOutcome,
};
use crate::application::remote_event_relay::RemoteConnectOutcome;
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

/// JS-facing projection of a pre-pair host identity probe (PR 2.5).
///
/// Descriptor truth only, and no credential of any kind: this response is produced
/// before any pairing code is consumed, so there is nothing secret to omit (P-18).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEnvironmentPreviewResponse {
    pub environment_id: String,
    pub app_version: String,
    pub platform: String,
    pub protocol_version: u32,
    pub min_client_protocol: u32,
    pub already_paired_as: Option<String>,
}

impl From<RemoteEnvironmentPreview> for RemoteEnvironmentPreviewResponse {
    fn from(preview: RemoteEnvironmentPreview) -> Self {
        Self {
            environment_id: preview.environment_id,
            app_version: preview.app_version,
            platform: preview.platform,
            protocol_version: preview.protocol_version,
            min_client_protocol: preview.min_client_protocol,
            already_paired_as: preview.already_paired_as,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRemoteEnvironmentInput {
    pub url: String,
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

/// Fetch seam, widened in PR 2.2 exactly as the PR 2.1 review-2 note required.
///
/// `backendFetch(path, init)` call sites POST JSON bodies and branch on
/// `res.ok`/`res.status`, so the seam carries `method`/`headers`/`body` in and a
/// `{ status, body }` envelope out. The webview's `method` and `headers` are
/// ALLOWLIST-validated in the service before a bearer is attached — this struct is
/// untrusted input, not a validated request.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFetchInput {
    pub id: String,
    pub path: String,
    /// Defaults to `GET` so the health/descriptor probes stay a two-field call.
    #[serde(default = "default_fetch_method")]
    pub method: String,
    /// Header pairs, allowlisted service-side (`content-type`, `accept`).
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<String>,
}

fn default_fetch_method() -> String {
    "GET".to_string()
}

fn service<'a>(state: &'a State<'_, AppState>) -> &'a RemoteEnvironmentService {
    &state.remote_environment_service
}

fn to_command_error(error: RemoteEnvironmentError) -> String {
    error.to_command_error()
}

/// Read-only host identity probe for the add-environment flow (PR 2.5).
///
/// Runs the same descriptor fetch and version-contradiction gate as
/// `pair_remote_environment`, so what the user is shown is what pairing will enforce.
/// Writes nothing: no row, no Keychain access, no active-environment change.
#[tauri::command]
pub async fn preview_remote_environment(
    input: PreviewRemoteEnvironmentInput,
    state: State<'_, AppState>,
) -> Result<RemoteEnvironmentPreviewResponse, String> {
    service(&state)
        .preview(&input.url)
        .await
        .map(RemoteEnvironmentPreviewResponse::from)
        .map_err(to_command_error)
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

/// Opens the Rust-owned outbound event socket for an environment (§3.2): bearer →
/// single-use ticket → dial → hello. The hello comes back as the outcome; the
/// stream frames themselves arrive as local `remote:stream_frame` events. The
/// socket — and the bearer/ticket — never reach JS (P-18).
#[tauri::command]
pub async fn remote_connect(
    input: RemoteEnvironmentIdInput,
    state: State<'_, AppState>,
) -> Result<RemoteConnectOutcome, String> {
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStreamSendInput {
    pub id: String,
    /// Typed as `ClientFrame` on purpose — the deserializer bounds what can be sent
    /// (`subscribe` / `cursorAck` / `heartbeatAck`); this is never a raw JSON
    /// passthrough to the socket.
    pub frame: ClientFrame,
}

/// Sends one protocol control frame on an environment's live event socket. The TS
/// `NetworkEventBus` owns the cursor (`afterSeq`, `cursorAck`) and speaks through
/// this command. Same authorization as `remote_connect` — background environments'
/// sockets stay drivable (§6.4); data/command paths remain active-env-bound (P-26).
#[tauri::command]
pub async fn remote_stream_send(
    input: RemoteStreamSendInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    service(&state)
        .stream_send(&input.id, input.frame)
        .await
        .map_err(to_command_error)
}

/// Forwards one command invoke through the Rust proxy (§6.3). Active-env-bound
/// (P-26); the bearer stays in Rust.
///
/// Two failure channels, deliberately: a HOST COMMAND that returned `Err(E)` resolves
/// with `RemoteInvokeOutcome::CommandError { error: E }` so `NetworkInvoke` can reject
/// with `E` verbatim (Tauri parity, §6.3), while a TRANSPORT failure takes the `Err`
/// channel as `"{CODE}: {message}"`.
#[tauri::command]
pub async fn remote_invoke(
    input: RemoteInvokeInput,
    state: State<'_, AppState>,
) -> Result<RemoteInvokeOutcome, String> {
    service(&state)
        .invoke(&input.id, &input.request_id, &input.cmd, input.args)
        .await
        .map_err(to_command_error)
}

/// Fetches a host resource through the Rust proxy (§3.5). Health paths (descriptor,
/// health probe) are allowed for background environments; everything else is
/// active-env-bound (P-26).
///
/// A non-2xx host answer is DATA, not an `Err`: `backendFetch` rebuilds a real
/// `Response` from `{status, body}` so migrated call sites keep reading `res.ok` and
/// their own error bodies. Only 401/403 lift into the taxonomy.
#[tauri::command]
pub async fn remote_fetch(
    input: RemoteFetchInput,
    state: State<'_, AppState>,
) -> Result<RemoteFetchOutcome, String> {
    service(&state)
        .fetch(
            &input.id,
            RemoteFetchCall {
                path: input.path,
                method: input.method,
                headers: input.headers,
                body: input.body,
            },
        )
        .await
        .map_err(to_command_error)
}

#[cfg(test)]
#[path = "remote_environment_commands_tests.rs"]
mod tests;
