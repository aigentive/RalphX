//! Spawn-free remote CONTINUATION of an existing idle conversation.
//!
//! This is the second half of "remote chat send" (chat-send design §4.1 Option 1). The first
//! half, `send_remote_chat_message`, only works while a run is LIVE — it queues into a queue
//! that only a live run drains. That left the remote surface ONE-SHOT: once the agent finished
//! its turn, a paired device could never speak again and got a dead-end
//! `REMOTE_CHAT_SEND_NOT_STEERABLE`. This module removes that.
//!
//! The contract this module keeps, and why it is its own module rather than a function in
//! `unified_chat_commands`:
//!
//! - It takes **no** `tauri::AppHandle`, **no** `ExecutionState`, and never builds a
//!   `ChatService`. Those are the three carriers of process-spawn authority, so their absence
//!   is mechanically checkable by reading this file (asserted by
//!   `the_spawn_free_remote_conversation_message_module_carries_no_authority_carriers`).
//! - Its only write is one durable intent row, the LAST statement (validate-then-persist,
//!   create-last). It arms no scheduler and starts no process; the host-owned
//!   `spawn_remote_conversation_message_dispatcher` loop is the sole holder of spawn authority.
//! - It refuses when a run IS live, and points the caller at `send_remote_chat_message`. The two
//!   surfaces are disjoint by construction, so a turn can never be both queued into a live run
//!   and dispatched as a fresh send.
//! - There is deliberately NO `role` field. The first-turn authorship is host-forced by FIELD
//!   ABSENCE, which is a stronger pin than a pinned value — there is nothing on the wire to forge.
//! - `model_override` / `logical_effort` DO travel (reassessment UX-5). The remote composer's
//!   options were previously dropped silently; they are now validated fail-closed against the
//!   conversation's own provider, rejected when unknown (never passed through to a CLI argv the
//!   way the local path's `normalize_agent_runtime_selection` does), and re-validated at dispatch.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{
    ChatContextType, ChatConversationId, ProjectId, RemoteConversationMessageRequest,
    RemoteConversationMessageStatus,
};

/// The message body was empty or whitespace only.
pub const REMOTE_CONV_MESSAGE_EMPTY_CONTENT: &str = "REMOTE_CONV_MESSAGE_EMPTY_CONTENT";
/// A precondition read failed. Deliberately distinct from "absent" so an unavailable
/// repository can never be mistaken for a satisfied precondition.
pub const REMOTE_CONV_MESSAGE_LOOKUP_FAILED: &str = "REMOTE_CONV_MESSAGE_LOOKUP_FAILED";
/// The named conversation does not exist on the host.
pub const REMOTE_CONV_MESSAGE_CONVERSATION_NOT_FOUND: &str =
    "REMOTE_CONV_MESSAGE_CONVERSATION_NOT_FOUND";
/// The conversation exists but is archived, so it may not be continued.
pub const REMOTE_CONV_MESSAGE_CONVERSATION_ARCHIVED: &str =
    "REMOTE_CONV_MESSAGE_CONVERSATION_ARCHIVED";
/// The conversation is not a project conversation owned by the named project. The scope comes
/// off the PERSISTED row, never off the request, so a client cannot aim a turn at a project it
/// does not name.
pub const REMOTE_CONV_MESSAGE_PROJECT_MISMATCH: &str = "REMOTE_CONV_MESSAGE_PROJECT_MISMATCH";
/// A run is already live for this conversation. The caller must use `send_remote_chat_message`,
/// which queues into the live run's queue — dispatching a fresh send alongside a live run would
/// double the turn.
pub const REMOTE_CONV_MESSAGE_RUN_ALREADY_LIVE: &str = "REMOTE_CONV_MESSAGE_RUN_ALREADY_LIVE";
/// The conversation's provider is unknown or no longer enabled on the host.
pub const REMOTE_CONV_MESSAGE_PROVIDER_NOT_ENABLED: &str =
    "REMOTE_CONV_MESSAGE_PROVIDER_NOT_ENABLED";
/// The requested `model_override` is not an enabled model for the conversation's provider.
pub const REMOTE_CONV_MESSAGE_MODEL_NOT_ENABLED: &str = "REMOTE_CONV_MESSAGE_MODEL_NOT_ENABLED";
/// The intent row could not be persisted.
pub const REMOTE_CONV_MESSAGE_ENQUEUE_FAILED: &str = "REMOTE_CONV_MESSAGE_ENQUEUE_FAILED";
/// The status read could not resolve the request.
pub const REMOTE_CONV_MESSAGE_REQUEST_NOT_FOUND: &str = "REMOTE_CONV_MESSAGE_REQUEST_NOT_FOUND";

/// Dispatcher-side: a run went live between persist and claim. Retryable — the client should
/// re-send through `send_remote_chat_message`, which is the surface that serves a live run.
pub const REMOTE_CONV_MESSAGE_RUN_WENT_LIVE: &str = "REMOTE_CONV_MESSAGE_RUN_WENT_LIVE";
/// Dispatcher-side: the host-side send itself failed. Terminal and VISIBLE — a persisted-but-
/// never-delivered turn must never look like a sent message (design doc §7).
pub const REMOTE_CONV_MESSAGE_HOST_SEND_FAILED: &str = "REMOTE_CONV_MESSAGE_HOST_SEND_FAILED";

/// Input for `request_remote_agent_conversation_message`.
///
/// There is deliberately NO `role`, `teamIntent`, `attachmentIds`, `suppressUserMessage`, or
/// composer-reference field: everything a remote device may not author is absent rather than
/// pinned, so there is nothing on the wire for a client to forge.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteAgentConversationMessageInput {
    pub conversation_id: String,
    pub project_id: String,
    pub content: String,
    pub model_override: Option<String>,
    pub logical_effort: Option<String>,
}

/// Result of persisting a continuation intent. Named for what the closure did (persisted a
/// request), not for what the host later does (send).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteAgentConversationMessageResponse {
    pub message_request_id: String,
    pub conversation_id: String,
    pub status: RemoteConversationMessageStatus,
    pub created_at: String,
}

/// The client's post-submit poll target.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConversationMessageRequestStatusView {
    pub id: String,
    pub conversation_id: String,
    pub status: RemoteConversationMessageStatus,
    pub error_code: Option<String>,
    pub agent_run_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Persist a continuation intent for an EXISTING idle conversation. No process is spawned here.
#[tauri::command]
pub async fn request_remote_agent_conversation_message(
    input: RequestRemoteAgentConversationMessageInput,
    state: State<'_, AppState>,
) -> Result<RequestRemoteAgentConversationMessageResponse, String> {
    request_remote_agent_conversation_message_for_state(&state, input).await
}

/// State-only body, so every precondition can be falsified through the production path.
pub async fn request_remote_agent_conversation_message_for_state(
    state: &AppState,
    input: RequestRemoteAgentConversationMessageInput,
) -> Result<RequestRemoteAgentConversationMessageResponse, String> {
    // 1. Content.
    let content = input.content.trim();
    if content.is_empty() {
        return Err(REMOTE_CONV_MESSAGE_EMPTY_CONTENT.to_string());
    }
    let content = content.to_string();

    // 2. Conversation — fail-closed tri-state (Err != None != Some).
    let conversation_id = ChatConversationId::from_string(input.conversation_id.trim().to_string());
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| {
            tracing::warn!(
                conversation_id = %conversation_id.as_str(),
                %error,
                "remote conversation message refused: conversation lookup failed"
            );
            REMOTE_CONV_MESSAGE_LOOKUP_FAILED.to_string()
        })?
        .ok_or_else(|| REMOTE_CONV_MESSAGE_CONVERSATION_NOT_FOUND.to_string())?;

    if conversation.is_archived() {
        return Err(REMOTE_CONV_MESSAGE_CONVERSATION_ARCHIVED.to_string());
    }

    // 3. Ownership. The scope is taken from the PERSISTED conversation and compared to the
    //    client's claim; a mismatch is a refusal, never a silent re-target.
    let requested_project = input.project_id.trim();
    if conversation.context_type != ChatContextType::Project
        || conversation.context_id != requested_project
    {
        return Err(REMOTE_CONV_MESSAGE_PROJECT_MISMATCH.to_string());
    }
    let project_id = ProjectId::from_string(conversation.context_id.clone());

    // 4. The project must still exist. `Err` is its own refusal, never "absent".
    if state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote conversation message refused: project lookup failed");
            REMOTE_CONV_MESSAGE_LOOKUP_FAILED.to_string()
        })?
        .is_none()
    {
        return Err(REMOTE_CONV_MESSAGE_PROJECT_MISMATCH.to_string());
    }

    // 5. Liveness — the disjointness guarantee. A live run means the caller wants the LIVE queue
    //    (`send_remote_chat_message`), not a fresh dispatch. Fail-closed in both directions: a
    //    read error is its own refusal, never "no run" (which would let this path dispatch
    //    alongside a live run) and never "live" (which would refuse a legitimate continuation).
    let active_run = state
        .agent_run_repo
        .get_active_for_conversation(&conversation_id)
        .await
        .map_err(|error| {
            tracing::warn!(
                conversation_id = %conversation_id.as_str(),
                %error,
                "remote conversation message refused: run liveness read failed"
            );
            REMOTE_CONV_MESSAGE_LOOKUP_FAILED.to_string()
        })?;
    if active_run.is_some() {
        return Err(REMOTE_CONV_MESSAGE_RUN_ALREADY_LIVE.to_string());
    }

    // 6. Provider — the conversation's own harness, or the enabled default when it has none yet.
    //    Never client-named: continuing a conversation must not silently switch its provider.
    let stored_providers = state
        .agent_provider_settings_repo
        .list()
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote conversation message refused: provider-settings lookup failed");
            REMOTE_CONV_MESSAGE_LOOKUP_FAILED.to_string()
        })?;
    let provider = resolve_conversation_provider(conversation.provider_harness, &stored_providers)?;

    // 7. Model/effort — STRICT: an unknown `model_override` is rejected, never passed through to
    //    a spawned CLI argv. Effort is clamped to the resolved model's supported set.
    let requested_effort = input
        .logical_effort
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<LogicalEffort>().ok());
    let requested_model = input
        .model_override
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (model_override, logical_effort) = match requested_model {
        Some(model_id) => {
            let snapshot = crate::commands::agent_model_commands::load_agent_model_registry(state)
                .await
                .map_err(|error| {
                    tracing::warn!(%error, "remote conversation message refused: model registry read failed");
                    REMOTE_CONV_MESSAGE_LOOKUP_FAILED.to_string()
                })?;
            let model_def = snapshot
                .find_enabled(provider, model_id)
                .ok_or_else(|| REMOTE_CONV_MESSAGE_MODEL_NOT_ENABLED.to_string())?;
            let effort = clamp_effort(
                requested_effort,
                &model_def.supported_efforts,
                model_def.default_effort,
            );
            (Some(model_id.to_string()), Some(effort.to_string()))
        }
        // No model named: the host resolves the conversation's own model at send time. The
        // requested effort travels as a NAME and is clamped by the send path.
        None => (None, requested_effort.map(|effort| effort.to_string())),
    };

    // 8. The intent row — the LAST write (validate-then-persist, create-last).
    let now = chrono::Utc::now();
    let request = RemoteConversationMessageRequest {
        id: uuid::Uuid::new_v4().to_string(),
        conversation_id: conversation_id.clone(),
        project_id,
        content,
        provider: provider.to_string(),
        model_override,
        logical_effort,
        status: RemoteConversationMessageStatus::Pending,
        error_code: None,
        // Deferred, exactly as for the start intent: stamping the authenticated device id
        // requires threading `identity.device_id` from the invoke layer through `dispatch`.
        // Revoke-cancel of pending intents rides on the same follow-up.
        requested_by_device_id: String::new(),
        agent_run_id: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    let stored = state
        .remote_conversation_message_request_repo
        .create_message_request(request)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote conversation message failed to persist intent");
            REMOTE_CONV_MESSAGE_ENQUEUE_FAILED.to_string()
        })?;

    Ok(RequestRemoteAgentConversationMessageResponse {
        message_request_id: stored.id,
        conversation_id: conversation_id.as_str(),
        status: stored.status,
        created_at: stored.created_at.to_rfc3339(),
    })
}

/// Poll one continuation intent by id. Pure repository read, fail-closed.
#[tauri::command]
pub async fn get_remote_conversation_message_request(
    message_request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteConversationMessageRequestStatusView, String> {
    get_remote_conversation_message_request_for_state(&state, &message_request_id).await
}

pub async fn get_remote_conversation_message_request_for_state(
    state: &AppState,
    message_request_id: &str,
) -> Result<RemoteConversationMessageRequestStatusView, String> {
    let request = state
        .remote_conversation_message_request_repo
        .get_message_request(message_request_id)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote conversation message status read failed");
            REMOTE_CONV_MESSAGE_LOOKUP_FAILED.to_string()
        })?
        .ok_or_else(|| REMOTE_CONV_MESSAGE_REQUEST_NOT_FOUND.to_string())?;

    Ok(RemoteConversationMessageRequestStatusView {
        id: request.id,
        conversation_id: request.conversation_id.as_str(),
        status: request.status,
        error_code: request.error_code,
        agent_run_id: request.agent_run_id,
        created_at: request.created_at.to_rfc3339(),
        updated_at: request.updated_at.to_rfc3339(),
    })
}

/// Resolve the harness that will serve this continuation. The conversation's own harness wins
/// and must still be enabled; a conversation that never bound one falls back to the stored
/// `enabled && is_default` row. Never client-named.
pub(crate) fn resolve_conversation_provider(
    conversation_harness: Option<AgentHarnessKind>,
    stored: &[ralphx_domain::agents::AgentProviderSettings],
) -> Result<AgentHarnessKind, String> {
    match conversation_harness {
        Some(kind) => {
            if stored.iter().any(|row| row.provider == kind && row.enabled) {
                Ok(kind)
            } else {
                Err(REMOTE_CONV_MESSAGE_PROVIDER_NOT_ENABLED.to_string())
            }
        }
        None => stored
            .iter()
            .find(|row| row.enabled && row.is_default)
            .map(|row| row.provider)
            .ok_or_else(|| REMOTE_CONV_MESSAGE_PROVIDER_NOT_ENABLED.to_string()),
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
#[path = "remote_conversation_message_commands_tests.rs"]
mod tests;
