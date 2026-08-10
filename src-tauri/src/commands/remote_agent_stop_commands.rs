//! Spawn-free remote agent STOP for the remote facade (WP2).
//!
//! `stop_agent` itself can never be registered: `AppChatService::stop_agent` drops the
//! `InteractiveProcess` and then reaches `running_agent_registry` -> `kill_process` ->
//! `Command::new(resolve_pkill_cli_path())`, which is a detector-(c) process-launch sink and
//! the process floor is absolute. That is a HYGIENE refusal, not a risk one — stopping is
//! authority-REDUCING, the same direction as `pause_execution`/`stop_execution`, which sit at
//! `ui:operate`.
//!
//! So the brake is redesigned rather than relaxed: this module persists a STOP INTENT, and a
//! host-owned dispatcher loop (`spawn_remote_agent_stop_dispatcher`) is the sole holder of the
//! pkill-resolving stop path. The request path resolves no CLI, spawns nothing, and — because
//! the intent is authority-reducing — is registered at **`ui:operate`** with an
//! `AUTHORITY_REDUCING_EXEMPTIONS` row, so the DEFAULT "viewer with brakes" pairing can halt a
//! runaway agent without ever being granted `ui:agent`.
//!
//! The contract this module keeps, and why it is its own module:
//!
//! - It takes **no** `tauri::AppHandle`, **no** `ExecutionState`, and never builds a
//!   `ChatService`. Those are the three carriers of process-spawn authority, so their absence
//!   is mechanically checkable by reading this file (asserted by
//!   `the_spawn_free_remote_agent_stop_module_carries_no_authority_carriers`).
//! - Its only write is one durable intent row reached through a repository leaf.
//! - The intent names a CONVERSATION, never a pid, run id, or process. What to terminate is
//!   resolved host-side at drain time, so a client cannot aim the brake at anything else.
//! - It is IDEMPOTENT per conversation: a second tap while a brake is already pending or
//!   in-flight returns that same request instead of stacking another. An intent queue that
//!   counted clicks would keep firing kills at a conversation the user has since restarted.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::entities::{ChatConversationId, RemoteAgentStopRequest, RemoteAgentStopStatus};

/// A precondition read failed. Deliberately distinct from "absent" so an unavailable
/// repository can never be mistaken for a satisfied precondition.
pub const REMOTE_AGENT_STOP_LOOKUP_FAILED: &str = "REMOTE_AGENT_STOP_LOOKUP_FAILED";
/// No conversation id was supplied.
pub const REMOTE_AGENT_STOP_EMPTY_CONVERSATION: &str = "REMOTE_AGENT_STOP_EMPTY_CONVERSATION";
/// The named conversation does not exist on this host.
pub const REMOTE_AGENT_STOP_CONVERSATION_NOT_FOUND: &str =
    "REMOTE_AGENT_STOP_CONVERSATION_NOT_FOUND";
/// The stop-intent row could not be persisted.
pub const REMOTE_AGENT_STOP_ENQUEUE_FAILED: &str = "REMOTE_AGENT_STOP_ENQUEUE_FAILED";
/// The status read could not resolve the request.
pub const REMOTE_AGENT_STOP_REQUEST_NOT_FOUND: &str = "REMOTE_AGENT_STOP_REQUEST_NOT_FOUND";
/// The host could not stop the run. Recorded by the dispatcher, surfaced through the poll.
pub const REMOTE_AGENT_STOP_HOST_STOP_FAILED: &str = "REMOTE_AGENT_STOP_HOST_STOP_FAILED";
/// The conversation vanished between persist and drain. Recorded by the dispatcher.
pub const REMOTE_AGENT_STOP_CONVERSATION_GONE: &str = "REMOTE_AGENT_STOP_CONVERSATION_GONE";

/// Input for `request_remote_agent_stop`.
///
/// There is deliberately no `contextType`, no run id, and no pid: the host pins the context to
/// the conversation it resolves, so the client cannot name a target outside the Agents surface.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteAgentStopInput {
    pub conversation_id: String,
}

/// Result of persisting a stop intent. Named for what the closure did (persisted a request),
/// not for what the host later does (stop).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteAgentStopResponse {
    pub stop_request_id: String,
    pub conversation_id: String,
    pub status: RemoteAgentStopStatus,
    /// `true` when this call joined an already-unsettled brake instead of creating a new one.
    pub deduplicated: bool,
    pub created_at: String,
}

/// The client's post-submit poll target.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAgentStopRequestStatusView {
    pub id: String,
    pub conversation_id: String,
    pub status: RemoteAgentStopStatus,
    pub error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Persist a stop intent for a host-run conversation. No process is touched here.
#[tauri::command]
pub async fn request_remote_agent_stop(
    input: RequestRemoteAgentStopInput,
    state: State<'_, AppState>,
) -> Result<RequestRemoteAgentStopResponse, String> {
    request_remote_agent_stop_for_state(&state, input).await
}

/// State-only body, so every precondition can be falsified through the production path.
pub async fn request_remote_agent_stop_for_state(
    state: &AppState,
    input: RequestRemoteAgentStopInput,
) -> Result<RequestRemoteAgentStopResponse, String> {
    // 1. Identity.
    let conversation_id = input.conversation_id.trim();
    if conversation_id.is_empty() {
        return Err(REMOTE_AGENT_STOP_EMPTY_CONVERSATION.to_string());
    }
    let conversation_id = ChatConversationId::from_string(conversation_id);

    // 2. The conversation must exist — fail-closed tri-state (Err != None != Some). A repo
    //    outage must not read as "unknown conversation" and must not read as "fine either".
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| {
            tracing::warn!(conversation_id = %conversation_id.as_str(), %error, "remote agent stop refused: conversation lookup failed");
            REMOTE_AGENT_STOP_LOOKUP_FAILED.to_string()
        })?
        .ok_or_else(|| REMOTE_AGENT_STOP_CONVERSATION_NOT_FOUND.to_string())?;
    let conversation_id = conversation.id.clone();

    // 3. Dedupe. A pending/in-flight brake for the same conversation is JOINED, never stacked.
    //    The read fails closed for the same reason as step 2: if we cannot prove there is no
    //    in-flight brake, minting a second one is the wrong default.
    let existing = state
        .remote_agent_stop_request_repo
        .find_unsettled_stop_request_for_conversation(&conversation_id)
        .await
        .map_err(|error| {
            tracing::warn!(conversation_id = %conversation_id.as_str(), %error, "remote agent stop refused: dedupe lookup failed");
            REMOTE_AGENT_STOP_LOOKUP_FAILED.to_string()
        })?;
    if let Some(existing) = existing {
        return Ok(RequestRemoteAgentStopResponse {
            stop_request_id: existing.id,
            conversation_id: conversation_id.as_str(),
            status: existing.status,
            deduplicated: true,
            created_at: existing.created_at.to_rfc3339(),
        });
    }

    // 4. The intent row — the LAST write (validate-then-persist, create-last).
    let now = chrono::Utc::now();
    let request = RemoteAgentStopRequest {
        id: uuid::Uuid::new_v4().to_string(),
        conversation_id: conversation_id.clone(),
        status: RemoteAgentStopStatus::Pending,
        error_code: None,
        // Deferred with the same rationale as the conversation-start intent: stamping the
        // authenticated device id needs `identity.device_id` threaded from the invoke layer
        // through `dispatch`. Left empty here; revoke-cancel rides the same follow-up.
        requested_by_device_id: String::new(),
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    let stored = state
        .remote_agent_stop_request_repo
        .create_stop_request(request)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote agent stop failed to persist intent");
            REMOTE_AGENT_STOP_ENQUEUE_FAILED.to_string()
        })?;

    Ok(RequestRemoteAgentStopResponse {
        stop_request_id: stored.id,
        conversation_id: conversation_id.as_str(),
        status: stored.status,
        deduplicated: false,
        created_at: stored.created_at.to_rfc3339(),
    })
}

/// Poll one stop intent by id. Pure repository read, fail-closed.
#[tauri::command]
pub async fn get_remote_agent_stop_request(
    stop_request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteAgentStopRequestStatusView, String> {
    get_remote_agent_stop_request_for_state(&state, &stop_request_id).await
}

pub async fn get_remote_agent_stop_request_for_state(
    state: &AppState,
    stop_request_id: &str,
) -> Result<RemoteAgentStopRequestStatusView, String> {
    let request = state
        .remote_agent_stop_request_repo
        .get_stop_request(stop_request_id)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote agent stop status read failed");
            REMOTE_AGENT_STOP_LOOKUP_FAILED.to_string()
        })?
        .ok_or_else(|| REMOTE_AGENT_STOP_REQUEST_NOT_FOUND.to_string())?;

    Ok(RemoteAgentStopRequestStatusView {
        id: request.id,
        conversation_id: request.conversation_id.as_str(),
        status: request.status,
        error_code: request.error_code,
        created_at: request.created_at.to_rfc3339(),
        updated_at: request.updated_at.to_rfc3339(),
    })
}

#[cfg(test)]
#[path = "remote_agent_stop_commands_tests.rs"]
mod tests;
