// Tauri commands for chat file attachments
//
// These commands handle file uploads, linking to messages, and managing
// attachments associated with chat conversations.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

use crate::application::builder_attachment_materializer::{
    materialize_builder_attachment, remove_materialized_builder_attachment_if_present,
    validate_builder_attachment_text,
};
use crate::application::chat_attachment_service::ChatAttachmentService;
use crate::application::AppState;
use crate::domain::entities::{ChatAttachmentId, ChatConversationId, ChatMessageId};
use crate::error::AppError;

// ============================================================================
// Request/Response types
// ============================================================================

/// Response for a chat attachment
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttachmentResponse {
    pub id: String,
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub file_name: String,
    pub file_path: String,
    pub mime_type: Option<String>,
    pub file_size: i64,
    pub created_at: String,
}

impl From<crate::domain::entities::ChatAttachment> for ChatAttachmentResponse {
    fn from(attachment: crate::domain::entities::ChatAttachment) -> Self {
        Self {
            id: attachment.id.as_str(),
            conversation_id: attachment.conversation_id.as_str(),
            message_id: attachment.message_id.map(|id| id.as_str().to_string()),
            file_name: attachment.file_name,
            file_path: attachment.file_path,
            mime_type: attachment.mime_type,
            file_size: attachment.file_size,
            created_at: attachment.created_at.to_rfc3339(),
        }
    }
}

/// Input for uploading a file attachment
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadChatAttachmentInput {
    pub conversation_id: String,
    pub file_name: String,
    pub file_data: Vec<u8>,
    pub mime_type: Option<String>,
}

/// Input for linking attachments to a message
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkAttachmentsInput {
    pub attachment_ids: Vec<String>,
    pub message_id: String,
}

// ============================================================================
// Commands
// ============================================================================

/// Upload a file attachment for a conversation
///
/// Creates a file in the app data directory and returns the attachment metadata.
/// The attachment is initially not linked to any message - use link_attachments_to_message
/// after the message is sent.
#[tauri::command]
pub async fn upload_chat_attachment(
    input: UploadChatAttachmentInput,
    state: State<'_, AppState>,
) -> Result<ChatAttachmentResponse, AppError> {
    upload_chat_attachment_for_state(input, state.inner()).await
}

#[doc(hidden)]
pub async fn upload_chat_attachment_for_state(
    input: UploadChatAttachmentInput,
    state: &AppState,
) -> Result<ChatAttachmentResponse, AppError> {
    let conversation_id = ChatConversationId::from_string(&input.conversation_id);
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Conversation {conversation_id} not found")))?;
    let is_builder = conversation.is_persona_builder();
    if is_builder {
        validate_builder_attachment_text(&input.file_data)?;
    }

    let service = ChatAttachmentService::new(
        Arc::clone(&state.chat_attachment_repo),
        state.attachment_storage_path.clone(),
    );
    let created = service
        .upload(
            &conversation_id,
            input.file_name,
            &input.file_data,
            input.mime_type,
        )
        .await?;
    if is_builder {
        if let Err(error) = materialize_builder_attachment(
            state.app_paths.app_data_dir(),
            &state.attachment_storage_path,
            &created,
        ) {
            if let Err(cleanup_error) = service.delete(&created.id).await {
                tracing::warn!(
                    attachment_id = %created.id,
                    %cleanup_error,
                    "Failed to remove attachment after builder materialization failed"
                );
            }
            return Err(error);
        }
    }

    Ok(ChatAttachmentResponse::from(created))
}

/// Link one or more attachments to a message (called after message is sent)
///
/// Updates the message_id field on attachments to associate them with a specific message.
#[tauri::command]
pub async fn link_attachments_to_message(
    input: LinkAttachmentsInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let attachment_ids: Vec<ChatAttachmentId> = input
        .attachment_ids
        .iter()
        .map(|id| ChatAttachmentId::from_string(id))
        .collect();

    let message_id = ChatMessageId::from_string(&input.message_id);

    state
        .chat_attachment_repo
        .update_message_ids(&attachment_ids, &message_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// List all attachments for a conversation
#[tauri::command]
pub async fn list_conversation_attachments(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ChatAttachmentResponse>, String> {
    let conversation_id = ChatConversationId::from_string(&conversation_id);

    let attachments = state
        .chat_attachment_repo
        .find_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(attachments
        .into_iter()
        .map(ChatAttachmentResponse::from)
        .collect())
}

/// List all attachments for a specific message
#[tauri::command]
pub async fn list_message_attachments(
    message_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ChatAttachmentResponse>, String> {
    let message_id = ChatMessageId::from_string(&message_id);

    let attachments = state
        .chat_attachment_repo
        .find_by_message_id(&message_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(attachments
        .into_iter()
        .map(ChatAttachmentResponse::from)
        .collect())
}

/// Delete a chat attachment (removes file and database record)
#[tauri::command]
pub async fn delete_chat_attachment(
    attachment_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    delete_chat_attachment_for_state(attachment_id, state.inner()).await
}

#[doc(hidden)]
pub async fn delete_chat_attachment_for_state(
    attachment_id: String,
    state: &AppState,
) -> Result<(), String> {
    let attachment_id = ChatAttachmentId::from_string(&attachment_id);
    let attachment = state
        .chat_attachment_repo
        .get_by_id(&attachment_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Attachment {} not found", attachment_id))?;
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&attachment.conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Conversation {} not found", attachment.conversation_id))?;
    if conversation.is_persona_builder() {
        remove_materialized_builder_attachment_if_present(
            state.app_paths.app_data_dir(),
            &attachment,
        )
        .map_err(|error| error.to_string())?;
    }

    ChatAttachmentService::new(
        Arc::clone(&state.chat_attachment_repo),
        state.attachment_storage_path.clone(),
    )
    .delete(&attachment_id)
    .await
    .map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "chat_attachment_commands_tests.rs"]
mod tests;
