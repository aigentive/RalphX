//! Spawn-free queued SEND-NOW intent. Only identifiers cross the remote wire; the host resolves
//! the queued payload and owns the later kill-and-launch transaction.
use crate::{
    application::AppState,
    domain::entities::{RemoteQueuedSendRequest, RemoteQueuedSendRequestStatus},
};
use serde::{Deserialize, Serialize};

pub const REMOTE_QUEUE_SEND_LOOKUP_FAILED: &str = "REMOTE_QUEUE_SEND_LOOKUP_FAILED";
pub const REMOTE_QUEUE_SEND_CONVERSATION_NOT_FOUND: &str =
    "REMOTE_QUEUE_SEND_CONVERSATION_NOT_FOUND";
pub const REMOTE_QUEUE_SEND_CONVERSATION_ARCHIVED: &str = "REMOTE_QUEUE_SEND_CONVERSATION_ARCHIVED";
pub const REMOTE_QUEUE_SEND_CONVERSATION_NOT_PROJECT: &str =
    "REMOTE_QUEUE_SEND_CONVERSATION_NOT_PROJECT";
pub const REMOTE_QUEUE_SEND_ENTRY_GONE: &str = "REMOTE_QUEUE_SEND_ENTRY_GONE";
pub const REMOTE_QUEUE_SEND_RUN_CHANGED: &str = "REMOTE_QUEUE_SEND_RUN_CHANGED";
pub const REMOTE_QUEUE_SEND_PROVIDER_DISABLED: &str = "REMOTE_QUEUE_SEND_PROVIDER_DISABLED";
pub const REMOTE_QUEUE_SEND_HOST_FAILED: &str = "REMOTE_QUEUE_SEND_HOST_FAILED";
pub const REMOTE_QUEUE_SEND_ALREADY_SENT: &str = "REMOTE_QUEUE_SEND_ALREADY_SENT";
pub const REMOTE_QUEUE_SEND_REQUEST_NOT_FOUND: &str = "REMOTE_QUEUE_SEND_REQUEST_NOT_FOUND";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteQueuedMessageSendInput {
    pub conversation_id: String,
    pub queued_message_id: String,
    pub expected_active_run_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteQueuedSendIntentResponse {
    pub request_id: String,
    pub status: RemoteQueuedSendRequestStatus,
    pub deduplicated: bool,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteQueuedSendRequestView {
    pub request_id: String,
    pub status: RemoteQueuedSendRequestStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn request_remote_queued_message_send_for_state(
    state: &AppState,
    input: RequestRemoteQueuedMessageSendInput,
) -> Result<RemoteQueuedSendIntentResponse, String> {
    use crate::domain::{
        entities::{ChatContextType, ChatConversationId},
        services::QueueKey,
    };
    let conversation_id = ChatConversationId::from_string(input.conversation_id.trim().to_string());
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|_| REMOTE_QUEUE_SEND_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_QUEUE_SEND_CONVERSATION_NOT_FOUND.to_string())?;
    if conversation.is_archived() {
        return Err(REMOTE_QUEUE_SEND_CONVERSATION_ARCHIVED.into());
    }
    if conversation.context_type != ChatContextType::Project {
        return Err(REMOTE_QUEUE_SEND_CONVERSATION_NOT_PROJECT.into());
    }
    let key = QueueKey::new(ChatContextType::Project, conversation_id.as_str());
    let mut listed = state
        .queued_message_repo
        .list(&key)
        .await
        .map_err(|_| REMOTE_QUEUE_SEND_LOOKUP_FAILED.to_string())?;
    listed.extend(
        state
            .message_queue
            .get_queued(ChatContextType::Project, &conversation_id.as_str()),
    );
    if !listed
        .iter()
        .any(|row| row.id == input.queued_message_id && !row.is_hidden_recovery())
    {
        return Err(REMOTE_QUEUE_SEND_ENTRY_GONE.into());
    }
    if let Some(expected) = input.expected_active_run_id.as_deref() {
        let active = state
            .agent_run_repo
            .get_active_for_conversation(&conversation_id)
            .await
            .map_err(|_| REMOTE_QUEUE_SEND_LOOKUP_FAILED.to_string())?;
        if active.as_ref().map(|run| run.id.as_str()).as_deref() != Some(expected) {
            return Err(REMOTE_QUEUE_SEND_RUN_CHANGED.into());
        }
    }
    if let Some(existing) = state
        .remote_queued_send_request_repo
        .find_unsettled(&conversation_id.as_str(), &input.queued_message_id)
        .await
        .map_err(|_| REMOTE_QUEUE_SEND_LOOKUP_FAILED.to_string())?
    {
        return Ok(response(existing, true));
    }
    let now = chrono::Utc::now();
    let row = RemoteQueuedSendRequest {
        id: uuid::Uuid::new_v4().to_string(),
        conversation_id: conversation_id.as_str(),
        queued_message_id: input.queued_message_id,
        expected_active_run_id: input.expected_active_run_id,
        status: RemoteQueuedSendRequestStatus::Pending,
        error_code: None,
        result: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    state
        .remote_queued_send_request_repo
        .create_remote_queued_send_request(row)
        .await
        .map(|row| response(row, false))
        .map_err(|_| REMOTE_QUEUE_SEND_LOOKUP_FAILED.into())
}
fn response(row: RemoteQueuedSendRequest, deduplicated: bool) -> RemoteQueuedSendIntentResponse {
    RemoteQueuedSendIntentResponse {
        request_id: row.id,
        status: row.status,
        deduplicated,
        created_at: row.created_at.to_rfc3339(),
    }
}
pub async fn get_remote_queued_message_send_request_for_state(
    state: &AppState,
    id: String,
) -> Result<RemoteQueuedSendRequestView, String> {
    let row = state
        .remote_queued_send_request_repo
        .get(&id)
        .await
        .map_err(|_| REMOTE_QUEUE_SEND_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_QUEUE_SEND_REQUEST_NOT_FOUND.to_string())?;
    Ok(RemoteQueuedSendRequestView {
        request_id: row.id,
        status: row.status,
        error_code: row.error_code,
        result: row.result,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    })
}
