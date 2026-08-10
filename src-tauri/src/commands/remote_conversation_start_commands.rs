//! Spawn-free remote conversation START for the remote facade.
//!
//! This is the registrable half of "create remotely, start on the host" (chat-send design
//! §4.2). A paired `ui:agent` device persists a START INTENT; a NEW host-owned dispatcher loop
//! (`spawn_remote_conversation_start_dispatcher`) is the sole holder of spawn authority and is
//! the only thing that ever calls `AgentConversationStartService::start`.
//!
//! The contract this module keeps, and why it is its own module rather than a function in
//! `unified_chat_commands`:
//!
//! - It takes **no** `tauri::AppHandle`, **no** `ExecutionState`, and never builds a
//!   `ChatService`. Those are the three carriers of process-spawn authority, so their absence
//!   is mechanically checkable by reading this file (asserted by
//!   `the_spawn_free_remote_conversation_start_module_carries_no_authority_carriers`).
//! - Its only writes are two durable rows — a seeded draft conversation and a start-intent —
//!   both reached through repository leaves. It arms no scheduler and starts no process.
//! - The model pin is STRICTER than the local path: an unknown `model_override` is REJECTED
//!   (`REMOTE_CONV_START_MODEL_NOT_ENABLED`), never passed through to a spawned CLI argv the
//!   way `normalize_agent_runtime_selection` does locally. Provider and mode are validated the
//!   same fail-closed way. `mode` is parsed into the host's known-mode enum before any write.
//!
//! Validate-then-persist, create-last: every precondition is a fail-closed tri-state
//! (`Err` != absent != ok), and the intent row is the LAST statement, mirroring the ideation
//! precedent (`ideation_commands_chat.rs`).

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, CoordinationMode, ProjectId,
    RemoteConversationStartRequest, RemoteConversationStartStatus,
};

/// The first-turn body was empty or whitespace only.
pub const REMOTE_CONV_START_EMPTY_CONTENT: &str = "REMOTE_CONV_START_EMPTY_CONTENT";
/// The requested mode is not a known `AgentConversationWorkspaceMode`.
pub const REMOTE_CONV_START_INVALID_MODE: &str = "REMOTE_CONV_START_INVALID_MODE";
/// A precondition read failed. Deliberately distinct from "absent" so an unavailable
/// repository can never be mistaken for a satisfied precondition.
pub const REMOTE_CONV_START_LOOKUP_FAILED: &str = "REMOTE_CONV_START_LOOKUP_FAILED";
/// The named project does not exist.
pub const REMOTE_CONV_START_PROJECT_NOT_FOUND: &str = "REMOTE_CONV_START_PROJECT_NOT_FOUND";
/// The named provider is unknown or not enabled on the host.
pub const REMOTE_CONV_START_PROVIDER_NOT_ENABLED: &str = "REMOTE_CONV_START_PROVIDER_NOT_ENABLED";
/// No provider is both enabled and marked default, so no default could be resolved.
pub const REMOTE_CONV_START_NO_DEFAULT_PROVIDER: &str = "REMOTE_CONV_START_NO_DEFAULT_PROVIDER";
/// The requested `model_override` is not an enabled model for the resolved provider. Unlike the
/// local path, an unknown model is REJECTED rather than passed through to the spawned CLI.
pub const REMOTE_CONV_START_MODEL_NOT_ENABLED: &str = "REMOTE_CONV_START_MODEL_NOT_ENABLED";
/// The start-intent row could not be persisted.
pub const REMOTE_CONV_START_ENQUEUE_FAILED: &str = "REMOTE_CONV_START_ENQUEUE_FAILED";
/// The seeded conversation could not be persisted.
pub const REMOTE_CONV_START_SEED_FAILED: &str = "REMOTE_CONV_START_SEED_FAILED";
/// The status read could not resolve the request.
pub const REMOTE_CONV_START_REQUEST_NOT_FOUND: &str = "REMOTE_CONV_START_REQUEST_NOT_FOUND";

/// Input for `request_remote_agent_conversation_start`.
///
/// `mode` is client-selected but host-validated against the known-mode enum. There is deliberately
/// NO `role`, `teamIntent`, or base/branch field — the first turn's authorship (`User`),
/// coordination (`Solo`), and git base are host-forced by FIELD ABSENCE.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteAgentConversationStartInput {
    pub project_id: String,
    pub content: String,
    pub title: Option<String>,
    pub provider: Option<String>,
    pub model_override: Option<String>,
    pub logical_effort: Option<String>,
    pub mode: String,
}

/// Result of persisting a start intent. Named for what the closure did (persisted a request),
/// not for what the host later does (start).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteAgentConversationStartResponse {
    pub start_request_id: String,
    pub conversation_id: String,
    pub status: RemoteConversationStartStatus,
    pub created_at: String,
}

/// The client's post-submit poll target.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConversationStartRequestStatusView {
    pub id: String,
    pub conversation_id: String,
    pub status: RemoteConversationStartStatus,
    pub error_code: Option<String>,
    pub agent_run_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Persist a start intent for a NEW host-run conversation. No process is spawned here.
#[tauri::command]
pub async fn request_remote_agent_conversation_start(
    input: RequestRemoteAgentConversationStartInput,
    state: State<'_, AppState>,
) -> Result<RequestRemoteAgentConversationStartResponse, String> {
    request_remote_agent_conversation_start_for_state(&state, input).await
}

/// State-only body, so every precondition can be falsified through the production path.
pub async fn request_remote_agent_conversation_start_for_state(
    state: &AppState,
    input: RequestRemoteAgentConversationStartInput,
) -> Result<RequestRemoteAgentConversationStartResponse, String> {
    // 1. Content.
    let content = input.content.trim();
    if content.is_empty() {
        return Err(REMOTE_CONV_START_EMPTY_CONTENT.to_string());
    }
    let content = content.to_string();

    // 2. The requested mode must be a KNOWN variant, exactly like the already-registered remote
    //    mode-switch intent. Parsing before project/provider reads keeps unknown values fail-closed
    //    and prevents them from reaching either persisted conversation state or the host spawner.
    let mode = input
        .mode
        .trim()
        .parse::<AgentConversationWorkspaceMode>()
        .map_err(|_| REMOTE_CONV_START_INVALID_MODE.to_string())?;

    // 3. Project — fail-closed tri-state (Err != None != Some).
    let requested_project = ProjectId::from_string(input.project_id.trim().to_string());
    let project = state
        .project_repo
        .get_by_id(&requested_project)
        .await
        .map_err(|error| {
            tracing::warn!(project_id = %requested_project.as_str(), %error, "remote conversation start refused: project lookup failed");
            REMOTE_CONV_START_LOOKUP_FAILED.to_string()
        })?
        .ok_or_else(|| REMOTE_CONV_START_PROJECT_NOT_FOUND.to_string())?;
    let project_id = project.id.clone();

    // 4. Provider — client-named validated against stored ENABLED rows, or the enabled default.
    let stored_providers = state
        .agent_provider_settings_repo
        .list()
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote conversation start refused: provider-settings lookup failed");
            REMOTE_CONV_START_LOOKUP_FAILED.to_string()
        })?;
    let provider = resolve_provider(&input.provider, &stored_providers)?;
    let stored_provider_row = stored_providers.iter().find(|row| row.provider == provider);

    // 5. Model — STRICT: an unknown `model_override` is rejected, never passed through. Effort
    //    is clamped to the resolved model's supported set.
    let snapshot = crate::commands::agent_model_commands::load_agent_model_registry(state)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote conversation start refused: model registry read failed");
            REMOTE_CONV_START_LOOKUP_FAILED.to_string()
        })?;
    let requested_effort = input
        .logical_effort
        .as_deref()
        .and_then(|value| value.parse::<LogicalEffort>().ok());
    let (model, effort) = match input.model_override.as_deref() {
        Some(model_id) => {
            let model_def = snapshot
                .find_enabled(provider, model_id)
                .ok_or_else(|| REMOTE_CONV_START_MODEL_NOT_ENABLED.to_string())?;
            let effort = clamp_effort(
                requested_effort,
                &model_def.supported_efforts,
                model_def.default_effort,
            );
            (Some(model_id.to_string()), Some(effort.to_string()))
        }
        None => {
            // Host default: prefer the provider's stored default model when it is still enabled,
            // else the registry default for the provider.
            let stored_default = stored_provider_row
                .and_then(|row| row.model.as_deref())
                .and_then(|model_id| snapshot.find_enabled(provider, model_id));
            let model_def = stored_default.or_else(|| snapshot.default_for_provider(provider));
            match model_def {
                Some(def) => {
                    let effort =
                        clamp_effort(requested_effort, &def.supported_efforts, def.default_effort);
                    (Some(def.model_id.clone()), Some(effort.to_string()))
                }
                // Provider is enabled but exposes no enabled model; leave the driver to resolve
                // at spawn time. The requested effort (if any) is carried as a name.
                None => (None, requested_effort.map(|effort| effort.to_string())),
            }
        }
    };

    // 6. Seed the conversation. Mirrors the Project branch of `create_agent_conversation`
    //    (unified_chat_commands/mod.rs), which is persist-only for the Project context (the
    //    standalone-workspace side effect is never taken here). Solo + User authorship are
    //    host-forced; mode is the validated client intent.
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.set_coordination_mode(CoordinationMode::Solo);
    conversation.set_agent_mode(Some(mode));
    if let Some(title) = input
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        conversation.set_title(title.to_string());
    }
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote conversation start failed to seed conversation");
            REMOTE_CONV_START_SEED_FAILED.to_string()
        })?;
    let conversation_id = conversation.id.clone();

    // 7. The intent row — the LAST write (validate-then-persist, create-last).
    let now = chrono::Utc::now();
    let request = RemoteConversationStartRequest {
        id: uuid::Uuid::new_v4().to_string(),
        conversation_id: conversation_id.clone(),
        project_id,
        content,
        provider: provider.to_string(),
        model,
        effort,
        mode: mode.to_string(),
        status: RemoteConversationStartStatus::Pending,
        error_code: None,
        // Deferred: stamping the authenticated device id requires threading `identity.device_id`
        // from the invoke layer through `dispatch` into a new injection arm. Left empty in v1.5;
        // revoke-cancel of pending intents (§8.3) rides on the same follow-up.
        requested_by_device_id: String::new(),
        agent_run_id: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    let stored = state
        .remote_conversation_start_request_repo
        .create_start_request(request)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote conversation start failed to persist intent");
            REMOTE_CONV_START_ENQUEUE_FAILED.to_string()
        })?;

    Ok(RequestRemoteAgentConversationStartResponse {
        start_request_id: stored.id,
        conversation_id: conversation_id.as_str(),
        status: stored.status,
        created_at: stored.created_at.to_rfc3339(),
    })
}

/// Poll one start intent by id. Pure repository read, fail-closed.
#[tauri::command]
pub async fn get_remote_conversation_start_request(
    start_request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteConversationStartRequestStatusView, String> {
    get_remote_conversation_start_request_for_state(&state, &start_request_id).await
}

pub async fn get_remote_conversation_start_request_for_state(
    state: &AppState,
    start_request_id: &str,
) -> Result<RemoteConversationStartRequestStatusView, String> {
    let request = state
        .remote_conversation_start_request_repo
        .get_start_request(start_request_id)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote conversation start status read failed");
            REMOTE_CONV_START_LOOKUP_FAILED.to_string()
        })?
        .ok_or_else(|| REMOTE_CONV_START_REQUEST_NOT_FOUND.to_string())?;

    Ok(RemoteConversationStartRequestStatusView {
        id: request.id,
        conversation_id: request.conversation_id.as_str(),
        status: request.status,
        error_code: request.error_code,
        agent_run_id: request.agent_run_id,
        created_at: request.created_at.to_rfc3339(),
        updated_at: request.updated_at.to_rfc3339(),
    })
}

/// Resolve the provider: a named provider must parse as a harness AND match a stored row with
/// `enabled == true`; `None` resolves to the `enabled && is_default` row.
fn resolve_provider(
    requested: &Option<String>,
    stored: &[ralphx_domain::agents::AgentProviderSettings],
) -> Result<AgentHarnessKind, String> {
    match requested {
        Some(name) => {
            let kind = name
                .parse::<AgentHarnessKind>()
                .map_err(|_| REMOTE_CONV_START_PROVIDER_NOT_ENABLED.to_string())?;
            if stored.iter().any(|row| row.provider == kind && row.enabled) {
                Ok(kind)
            } else {
                Err(REMOTE_CONV_START_PROVIDER_NOT_ENABLED.to_string())
            }
        }
        None => stored
            .iter()
            .find(|row| row.enabled && row.is_default)
            .map(|row| row.provider)
            .ok_or_else(|| REMOTE_CONV_START_NO_DEFAULT_PROVIDER.to_string()),
    }
}

/// Clamp a requested effort to the model's supported set, falling back to its default. Unlike
/// the model branch, clamping an effort is safe — a nonsense effort can never reach spawn args.
fn clamp_effort(
    requested: Option<LogicalEffort>,
    supported: &[LogicalEffort],
    default: LogicalEffort,
) -> LogicalEffort {
    requested
        .filter(|effort| supported.contains(effort))
        .unwrap_or(default)
}

#[cfg(test)]
#[path = "remote_conversation_start_commands_tests.rs"]
mod tests;
