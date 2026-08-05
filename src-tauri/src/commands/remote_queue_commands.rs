//! Spawn-free queued-message reads and authority-reducing deletes for the remote facade.

use serde::Serialize;
use tauri::State;

use crate::application::AppState;
use crate::commands::unified_chat_commands::{
    delete_queued_agent_message_for_state, list_queued_agent_messages_for_state,
    QueuedMessageResponse,
};
use crate::domain::entities::{ChatContextType, ChatConversationId};

pub const REMOTE_QUEUE_LOOKUP_FAILED: &str = "REMOTE_QUEUE_LOOKUP_FAILED";
pub const REMOTE_QUEUE_CONVERSATION_NOT_FOUND: &str = "REMOTE_QUEUE_CONVERSATION_NOT_FOUND";
pub const REMOTE_QUEUE_CONVERSATION_ARCHIVED: &str = "REMOTE_QUEUE_CONVERSATION_ARCHIVED";
pub const REMOTE_QUEUE_CONVERSATION_NOT_PROJECT: &str = "REMOTE_QUEUE_CONVERSATION_NOT_PROJECT";

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelRemoteQueuedAgentMessageResponse {
    pub deleted: bool,
}

pub(crate) async fn validate_remote_queue_conversation(
    state: &AppState,
    conversation_id: &str,
) -> Result<ChatConversationId, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id.trim().to_string());
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote queue conversation lookup failed");
            REMOTE_QUEUE_LOOKUP_FAILED.to_string()
        })?
        .ok_or_else(|| REMOTE_QUEUE_CONVERSATION_NOT_FOUND.to_string())?;
    if conversation.is_archived() {
        return Err(REMOTE_QUEUE_CONVERSATION_ARCHIVED.to_string());
    }
    if conversation.context_type != ChatContextType::Project {
        return Err(REMOTE_QUEUE_CONVERSATION_NOT_PROJECT.to_string());
    }
    Ok(conversation.id)
}

pub use crate::application::remote_queue_send_intent::*;

#[tauri::command]
pub async fn request_remote_queued_message_send(
    conversation_id: String,
    queued_message_id: String,
    expected_active_run_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<RemoteQueuedSendIntentResponse, String> {
    request_remote_queued_message_send_for_state(
        &state,
        RequestRemoteQueuedMessageSendInput {
            conversation_id,
            queued_message_id,
            expected_active_run_id,
        },
    )
    .await
}

#[tauri::command]
pub async fn get_remote_queued_message_send_request(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteQueuedSendRequestView, String> {
    get_remote_queued_message_send_request_for_state(&state, request_id).await
}

#[tauri::command]
pub async fn list_remote_queued_agent_messages(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<QueuedMessageResponse>, String> {
    list_remote_queued_agent_messages_for_state(&state, &conversation_id).await
}

#[doc(hidden)]
pub async fn list_remote_queued_agent_messages_for_state(
    state: &AppState,
    conversation_id: &str,
) -> Result<Vec<QueuedMessageResponse>, String> {
    let conversation_id = validate_remote_queue_conversation(state, conversation_id).await?;
    list_queued_agent_messages_for_state(state, ChatContextType::Project, &conversation_id.as_str())
        .await
}

#[tauri::command]
pub async fn cancel_remote_queued_agent_message(
    conversation_id: String,
    message_id: String,
    state: State<'_, AppState>,
) -> Result<CancelRemoteQueuedAgentMessageResponse, String> {
    cancel_remote_queued_agent_message_for_state(&state, &conversation_id, &message_id).await
}

#[doc(hidden)]
pub async fn cancel_remote_queued_agent_message_for_state(
    state: &AppState,
    conversation_id: &str,
    message_id: &str,
) -> Result<CancelRemoteQueuedAgentMessageResponse, String> {
    let conversation_id = validate_remote_queue_conversation(state, conversation_id).await?;
    let deleted = delete_queued_agent_message_for_state(
        state,
        ChatContextType::Project,
        &conversation_id.as_str(),
        message_id,
    )
    .await?;
    Ok(CancelRemoteQueuedAgentMessageResponse { deleted })
}
