//! Spawn-free chat send for the remote facade.
//!
//! This module exists so a paired `ui:agent` device can steer a conversation the host
//! already started, without the request path ever reaching a process-launch sink.
//!
//! Design: `docs/handoffs/remote-mobile/chat-send-spawn-free-design.md` §4.1 Option 2.
//!
//! The contract this module must keep, and the reason it is a module of its own rather
//! than a function inside `unified_chat_commands`:
//!
//! - It takes **no** `tauri::AppHandle` and **no** `ExecutionState`, and never builds a
//!   `ChatService`. Those are the three carriers of process-spawn authority, so their
//!   absence is mechanically checkable by reading this file.
//! - Its only write is a durable queued-message row. That row is drained solely by
//!   `chat_service_queue::process_queued_messages`, which runs *inside an already-live
//!   run*. It arms no scheduler and starts nothing.
//! - Because the row is only ever drained by a live run, a send with no live run would be
//!   persisted and never delivered — a false-success. The liveness precondition below is
//!   what prevents that, and it is fail-closed in both directions: a repository error is
//!   its own refusal, never "no run" and never "assume live".

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::services::{QueueKey, QueuedMessage};
use ralphx_domain::entities::{ChatConversationId, MessageRole};

/// The conversation id did not resolve to a persisted conversation.
pub const REMOTE_CHAT_SEND_CONVERSATION_NOT_FOUND: &str = "REMOTE_CHAT_SEND_CONVERSATION_NOT_FOUND";
/// The conversation exists but is archived, so it may not be steered.
pub const REMOTE_CHAT_SEND_CONVERSATION_ARCHIVED: &str = "REMOTE_CHAT_SEND_CONVERSATION_ARCHIVED";
/// No live run owns this conversation, so a queued message would never be delivered.
pub const REMOTE_CHAT_SEND_NOT_STEERABLE: &str = "REMOTE_CHAT_SEND_NOT_STEERABLE";
/// A role other than `user` was requested. Only user turns may be authored remotely.
pub const REMOTE_CHAT_SEND_ROLE_NOT_PERMITTED: &str = "REMOTE_CHAT_SEND_ROLE_NOT_PERMITTED";
/// The message body was empty or whitespace only.
pub const REMOTE_CHAT_SEND_EMPTY_CONTENT: &str = "REMOTE_CHAT_SEND_EMPTY_CONTENT";
/// A precondition read failed. Deliberately distinct from "absent" so an unavailable
/// repository can never be mistaken for a satisfied precondition.
pub const REMOTE_CHAT_SEND_LOOKUP_FAILED: &str = "REMOTE_CHAT_SEND_LOOKUP_FAILED";
/// The queued row could not be persisted.
pub const REMOTE_CHAT_SEND_ENQUEUE_FAILED: &str = "REMOTE_CHAT_SEND_ENQUEUE_FAILED";

/// Input for `send_remote_chat_message`.
///
/// `role` is present so the facade's `PinnedField` machinery has a field to pin. The
/// wire value is overwritten with `"user"` at dispatch before this struct is built, and
/// the command below independently rejects anything else — two failures are required for
/// a forged speaker label to reach the transcript.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendRemoteChatMessageInput {
    pub conversation_id: String,
    pub content: String,
    pub role: String,
}

/// Result of a spawn-free remote send.
///
/// The field is named `queued_message_id`, not `message_id`: the message is queued for an
/// already-running agent, and calling it "sent" would overstate what happened.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SendRemoteChatMessageResponse {
    pub conversation_id: String,
    pub queued_message_id: String,
    pub agent_run_id: String,
    pub created_at: String,
}

/// Queue a user turn into a conversation that a live run is already serving.
#[tauri::command]
pub async fn send_remote_chat_message(
    input: SendRemoteChatMessageInput,
    state: State<'_, AppState>,
) -> Result<SendRemoteChatMessageResponse, String> {
    send_remote_chat_message_for_state(&state, input).await
}

/// State-only body, so the preconditions can be falsified in tests through the same path
/// production uses.
pub async fn send_remote_chat_message_for_state(
    state: &AppState,
    input: SendRemoteChatMessageInput,
) -> Result<SendRemoteChatMessageResponse, String> {
    let role: MessageRole = input
        .role
        .parse()
        .map_err(|_| REMOTE_CHAT_SEND_ROLE_NOT_PERMITTED.to_string())?;
    if role != MessageRole::User {
        return Err(REMOTE_CHAT_SEND_ROLE_NOT_PERMITTED.to_string());
    }

    let content = input.content.trim();
    if content.is_empty() {
        return Err(REMOTE_CHAT_SEND_EMPTY_CONTENT.to_string());
    }

    let conversation_id = ChatConversationId::from_string(input.conversation_id);

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| {
            tracing::warn!(
                conversation_id = %conversation_id.as_str(),
                error = %error,
                "remote chat send refused: conversation lookup failed"
            );
            REMOTE_CHAT_SEND_LOOKUP_FAILED.to_string()
        })?
        .ok_or_else(|| REMOTE_CHAT_SEND_CONVERSATION_NOT_FOUND.to_string())?;

    if conversation.is_archived() {
        return Err(REMOTE_CHAT_SEND_CONVERSATION_ARCHIVED.to_string());
    }

    // The whole security surface of this command. An error here must not read as "no run"
    // (which would merely refuse) nor as "live" (which would let a message rot in a queue
    // nothing drains). It gets its own terminal refusal.
    let active_run = state
        .agent_run_repo
        .get_active_for_conversation(&conversation_id)
        .await
        .map_err(|error| {
            tracing::warn!(
                conversation_id = %conversation_id.as_str(),
                error = %error,
                "remote chat send refused: run liveness read failed"
            );
            REMOTE_CHAT_SEND_LOOKUP_FAILED.to_string()
        })?
        .ok_or_else(|| REMOTE_CHAT_SEND_NOT_STEERABLE.to_string())?;

    // The queue key comes off the persisted conversation, never off the request, so a
    // client cannot aim a message at a context it did not name.
    let queue_key = QueueKey::new(conversation.context_type, conversation.context_id.as_str());
    let queued = QueuedMessage::with_id(uuid::Uuid::new_v4().to_string(), content.to_string());

    state
        .queued_message_repo
        .enqueue_back(&queue_key, &queued)
        .await
        .map_err(|error| {
            tracing::warn!(
                conversation_id = %conversation_id.as_str(),
                error = %error,
                "remote chat send failed to persist queued row"
            );
            REMOTE_CHAT_SEND_ENQUEUE_FAILED.to_string()
        })?;

    // Durable row first, in-memory mirror second — the same order
    // `question_commands` uses. The live run reads memory first and falls back to the
    // durable row, so a crash between the two loses nothing.
    state.message_queue.queue_back_existing(
        conversation.context_type,
        &conversation.context_id,
        queued.clone(),
    );

    Ok(SendRemoteChatMessageResponse {
        conversation_id: conversation_id.as_str(),
        queued_message_id: queued.id,
        agent_run_id: active_run.id.as_str().to_string(),
        created_at: queued.created_at,
    })
}
