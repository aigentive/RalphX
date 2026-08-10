//! Host attachment metadata reads for the remote facade.
//!
//! The local attachment module also owns upload/delete filesystem authority. This twin drops
//! those carriers entirely: no `ChatAttachmentService`, attachment storage path, `AppHandle`,
//! or `ExecutionState` enters this module. It reads only the attachment repository.
//!
//! [`RemoteChatAttachmentResponse`] deliberately omits `file_path`. A host path cannot be
//! opened by a paired client and putting it on the wire would be both misleading and an
//! invitation to send host filesystem authority back through another surface. Attachment
//! bytes remain host-only in this slice.

use serde::Serialize;
use tauri::State;

use crate::application::AppState;
use crate::domain::entities::{ChatAttachment, ChatMessageId};

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteChatAttachmentResponse {
    pub id: String,
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub file_name: String,
    pub mime_type: Option<String>,
    pub file_size: i64,
    pub created_at: String,
}

impl From<ChatAttachment> for RemoteChatAttachmentResponse {
    fn from(attachment: ChatAttachment) -> Self {
        Self {
            id: attachment.id.as_str(),
            conversation_id: attachment.conversation_id.as_str(),
            message_id: attachment.message_id.map(|id| id.as_str().to_string()),
            file_name: attachment.file_name,
            mime_type: attachment.mime_type,
            file_size: attachment.file_size,
            created_at: attachment.created_at.to_rfc3339(),
        }
    }
}

#[tauri::command]
pub async fn list_remote_message_attachments(
    message_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<RemoteChatAttachmentResponse>, String> {
    list_remote_message_attachments_for_state(state.inner(), &message_id).await
}

#[doc(hidden)]
pub async fn list_remote_message_attachments_for_state(
    state: &AppState,
    message_id: &str,
) -> Result<Vec<RemoteChatAttachmentResponse>, String> {
    let message_id = ChatMessageId::from_string(message_id);
    state
        .chat_attachment_repo
        .find_by_message_id(&message_id)
        .await
        .map(|attachments| attachments.into_iter().map(Into::into).collect())
        .map_err(|error| error.to_string())
}
