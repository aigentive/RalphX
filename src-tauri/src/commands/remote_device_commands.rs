//! Host-local Tauri commands for remote pairing, devices, and sessions (§5.4).
//!
//! None of these is reachable on :3849. There is no remote `admin` scope: device management
//! and pairing-code minting are Tauri-command-only on the host and compile-denied from the
//! invoke facade (§3.1, §4.4). The raw pairing code exists only in the mint response the
//! host's own UI renders as a QR/URL — it is stored hashed (A-9).

use chrono::Utc;
use ralphx_remote_protocol::{ResetReason, Scope};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::domain::entities::{
    validate_pairing_grant, RemoteAuditAction, RemoteDevice, RemoteDeviceId, RemotePairingCode,
    RemotePairingCodeId, RemoteScopeSet, RemoteSessionId,
};
use crate::domain::services::key_crypto::hash_key;
use crate::remote_server::auth::{
    expiry_timestamp, generate_pairing_code, now_timestamp, RemoteAuthContext,
    PAIRING_CODE_TTL_SECS,
};
use crate::remote_server::remote_listener_handle;
use crate::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MintedRemotePairingCode {
    pub id: String,
    /// Shown once by the host UI as a QR/`ralphx://pair` URL. Never persisted in the clear.
    pub code: String,
    pub scopes: Vec<Scope>,
    pub created_at: String,
    pub expires_at: String,
    pub expires_in_secs: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePairingCodeView {
    pub id: String,
    pub scopes: Vec<Scope>,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDeviceView {
    pub id: String,
    pub name: String,
    pub token_prefix: String,
    pub scopes: Vec<Scope>,
    pub agent_control_granted: bool,
    pub created_at: String,
    pub last_seen_at: Option<String>,
    pub revoked_at: Option<String>,
    pub live_session_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSessionView {
    pub id: String,
    pub device_id: String,
    pub connected_at: String,
    pub last_active_at: String,
    pub remote_addr: String,
    /// Whether the in-memory registry still holds a kill channel for this session.
    pub live: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDeviceIdInput {
    pub device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetRemoteDeviceAgentControlInput {
    pub device_id: String,
    /// Off by default; this is the only way `ui:agent` is ever granted (§5.4).
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePairingCodeIdInput {
    pub id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSessionIdInput {
    pub session_id: String,
}

/// Builds the host-local view of the remote-access stores, sharing the process-wide registry.
fn remote_context(state: &State<'_, AppState>, app: &tauri::AppHandle) -> RemoteAuthContext {
    let handle = remote_listener_handle(app);
    RemoteAuthContext::host_local(state.db.clone(), handle.sessions().clone())
}

fn device_view(device: &RemoteDevice, live_session_count: usize) -> RemoteDeviceView {
    RemoteDeviceView {
        id: device.id.to_string(),
        name: device.name.clone(),
        token_prefix: device.token_prefix.clone(),
        scopes: device.scopes.to_vec(),
        agent_control_granted: device.agent_control_granted(),
        created_at: device.created_at.clone(),
        last_seen_at: device.last_seen_at.clone(),
        revoked_at: device.revoked_at.clone(),
        live_session_count,
    }
}

/// Mints a single-use pairing code with a 10-minute TTL.
///
/// The grant is validated first: a pairing code can never carry `ui:agent` or `ui:elevated`,
/// so agent control cannot be smuggled in through pairing (§4.3).
#[tauri::command]
pub async fn generate_remote_pairing_code(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<MintedRemotePairingCode, String> {
    let context = remote_context(&state, &app);
    let scopes = RemoteScopeSet::default_pairing_grant();
    validate_pairing_grant(&scopes).map_err(|error| error.to_string())?;

    let raw_code = generate_pairing_code();
    let minted_at = Utc::now();
    let created_at = now_timestamp();
    let expires_at = expiry_timestamp(minted_at, PAIRING_CODE_TTL_SECS);
    let stored = context
        .pairing_codes
        .create(RemotePairingCode {
            id: RemotePairingCodeId::new(),
            code_hash: hash_key(&raw_code),
            scopes: scopes.clone(),
            created_at: created_at.clone(),
            expires_at: expires_at.clone(),
            consumed_at: None,
        })
        .await
        .map_err(|error| error.to_string())?;
    // A pairing code is an access grant, so `record_audit`'s contract applies: a grant whose
    // decision left no trail is refused. The code is cancelled first so the failure does not
    // leave a live, redeemable, unaudited code behind.
    if !context
        .record_audit(None, RemoteAuditAction::PairingCodeCreated, None)
        .await
    {
        if let Err(error) = context
            .pairing_codes
            .cancel(&stored.id, &now_timestamp())
            .await
        {
            tracing::error!(%error, "Cancelling an unaudited pairing code failed");
        }
        return Err("The pairing code could not be recorded in the audit log.".to_string());
    }

    Ok(MintedRemotePairingCode {
        id: stored.id.to_string(),
        code: raw_code,
        scopes: scopes.to_vec(),
        created_at,
        expires_at,
        expires_in_secs: PAIRING_CODE_TTL_SECS,
    })
}

/// Outstanding (unconsumed, unexpired) codes, without their raw values.
#[tauri::command]
pub async fn list_remote_pairing_codes(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Vec<RemotePairingCodeView>, String> {
    let context = remote_context(&state, &app);
    let codes = context
        .pairing_codes
        .list_outstanding(&now_timestamp())
        .await
        .map_err(|error| error.to_string())?;
    Ok(codes
        .into_iter()
        .map(|code| RemotePairingCodeView {
            id: code.id.to_string(),
            scopes: code.scopes.to_vec(),
            created_at: code.created_at,
            expires_at: code.expires_at,
        })
        .collect())
}

/// Cancels an outstanding pairing code before anyone redeems it.
#[tauri::command]
pub async fn revoke_remote_pairing_code(
    input: RemotePairingCodeIdInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let context = remote_context(&state, &app);
    let cancelled = context
        .pairing_codes
        .cancel(
            &RemotePairingCodeId::from_string(input.id),
            &now_timestamp(),
        )
        .await
        .map_err(|error| error.to_string())?;
    if cancelled {
        context
            .record_audit(None, RemoteAuditAction::PairingCodeRevoked, None)
            .await;
    }
    Ok(cancelled)
}

/// Every paired device, revoked ones included, with its live session count.
#[tauri::command]
pub async fn list_remote_devices(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Vec<RemoteDeviceView>, String> {
    let context = remote_context(&state, &app);
    let devices = context
        .devices
        .list()
        .await
        .map_err(|error| error.to_string())?;
    Ok(devices
        .iter()
        .map(|device| device_view(device, context.registry.device_session_count(&device.id)))
        .collect())
}

/// Grants or withdraws remote agent control for one device.
///
/// Withdrawing narrows the durable grant **first**, then fires the device's kill channels, so
/// a live session can never outlive the authority it was admitted under (§4.4, §5.4).
#[tauri::command]
pub async fn set_remote_device_agent_control(
    input: SetRemoteDeviceAgentControlInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<RemoteDeviceView, String> {
    let context = remote_context(&state, &app);
    let device_id = RemoteDeviceId::from_string(input.device_id);
    let device = context
        .devices
        .get(&device_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "This remote device no longer exists.".to_string())?;
    // `set_scopes` silently updates zero rows for a revoked device (`revoked_at IS NULL`
    // guard), which would let this command audit a grant that never happened. Refuse
    // explicitly instead.
    if device.revoked_at.is_some() {
        return Err(
            "This remote device is revoked; re-pair it to change agent control.".to_string(),
        );
    }

    let scopes = if input.enabled {
        device.scopes.with(Scope::UiAgent)
    } else {
        device.scopes.without(Scope::UiAgent)
    };
    let updated = context
        .devices
        .set_scopes(&device_id, &scopes)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "This remote device no longer exists.".to_string())?;

    context
        .record_audit(
            Some(&device_id),
            if input.enabled {
                RemoteAuditAction::AgentControlGranted
            } else {
                RemoteAuditAction::AgentControlRevoked
            },
            None,
        )
        .await;

    // Narrowing is the only direction that needs teardown: a session admitted while agent
    // control was on must not keep running under the old grant.
    //
    // `Revoked` here is the closest member of the pinned v1 reset vocabulary; the DEVICE is
    // still paired and its token still valid. PR 1.4/2.3 must therefore treat `reset(revoked)`
    // as advisory: reconnect and re-run `GET /remote/v1/session` (P-28) before entering the
    // terminal credentials-blocked state — only an introspection 401 proves the token died.
    if !input.enabled {
        let torn_down = context
            .tear_down_device_sessions(&device_id, ResetReason::Revoked)
            .await;
        tracing::info!(
            device_id = %device_id,
            sessions = torn_down,
            "Remote agent control withdrawn"
        );
    }

    let live = context.registry.device_session_count(&device_id);
    Ok(device_view(&updated, live))
}

/// Revokes a device: writes `revoked_at`, then tears its live sessions down immediately.
#[tauri::command]
pub async fn revoke_remote_device(
    input: RemoteDeviceIdInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<RemoteDeviceView, String> {
    let context = remote_context(&state, &app);
    let device_id = RemoteDeviceId::from_string(input.device_id);
    let revoked = context
        .devices
        .revoke(&device_id, &now_timestamp())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "This remote device no longer exists.".to_string())?;

    context
        .record_audit(Some(&device_id), RemoteAuditAction::DeviceRevoked, None)
        .await;
    let torn_down = context
        .tear_down_device_sessions(&device_id, ResetReason::Revoked)
        .await;
    tracing::info!(device_id = %device_id, sessions = torn_down, "Remote device revoked");

    Ok(device_view(
        &revoked,
        context.registry.device_session_count(&device_id),
    ))
}

/// Open session rows, flagged with whether the registry still holds them.
#[tauri::command]
pub async fn list_remote_sessions(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Vec<RemoteSessionView>, String> {
    let context = remote_context(&state, &app);
    let sessions = context
        .sessions
        .list_open()
        .await
        .map_err(|error| error.to_string())?;
    Ok(sessions
        .into_iter()
        .map(|session| {
            let live = context
                .registry
                .live_sessions(&session.device_id)
                .contains(&session.id);
            RemoteSessionView {
                id: session.id.to_string(),
                device_id: session.device_id.to_string(),
                connected_at: session.connected_at,
                last_active_at: session.last_active_at,
                remote_addr: session.remote_addr,
                live,
            }
        })
        .collect())
}

/// Closes one live session without revoking its device.
#[tauri::command]
pub async fn disconnect_remote_session(
    input: RemoteSessionIdInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let context = remote_context(&state, &app);
    let session_id = RemoteSessionId::from_string(input.session_id);
    // The owning device comes from the durable row, never from the caller, so a stale id
    // cannot be used to signal a different device's sessions.
    let session = context
        .sessions
        .list_open()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|session| session.id == session_id);
    let Some(session) = session else {
        return Ok(false);
    };

    let now = now_timestamp();
    context
        .sessions
        .close(&session_id, &now)
        .await
        .map_err(|error| error.to_string())?;
    context
        .registry
        .kill_session(&session.device_id, &session_id, ResetReason::HostDisabled);
    context
        .record_audit(
            Some(&session.device_id),
            RemoteAuditAction::SessionClosed,
            Some("disconnected by the host owner"),
        )
        .await;
    Ok(true)
}
