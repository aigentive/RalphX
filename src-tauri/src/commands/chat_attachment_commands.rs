// Tauri commands for chat file attachments
//
// These commands handle file uploads, linking to messages, and managing
// attachments associated with chat conversations.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

use crate::application::chat_attachment_storage::build_chat_attachment_file_path;
use crate::application::AppState;
use crate::domain::entities::{
    ChatAttachment, ChatAttachmentId, ChatConversationId, ChatMessageId,
};

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
) -> Result<ChatAttachmentResponse, String> {
    let conversation_id = ChatConversationId::from_string(&input.conversation_id);
    let file_size = input.file_data.len() as i64;

    // Generate attachment ID and determine storage path
    let attachment_id = ChatAttachmentId::new();
    let file_path = build_chat_attachment_file_path(
        &state.attachment_storage_path,
        &conversation_id,
        &attachment_id,
        &input.file_name,
    )?;

    // Create parent directories
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    // Write file to disk
    std::fs::write(&file_path, &input.file_data)
        .map_err(|e| format!("Failed to write file: {}", e))?;

    // Create database record
    let attachment = ChatAttachment::new(
        conversation_id,
        input.file_name,
        file_path.to_string_lossy().to_string(),
        file_size,
        input.mime_type,
    );

    let created = state
        .chat_attachment_repo
        .create(attachment)
        .await
        .map_err(|e| e.to_string())?;

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
    let attachment_id = ChatAttachmentId::from_string(&attachment_id);

    // Get attachment to find file path
    let attachment = state
        .chat_attachment_repo
        .get_by_id(&attachment_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Attachment {} not found", attachment_id))?;

    // Delete file from disk (ignore errors if file doesn't exist)
    let file_path = PathBuf::from(&attachment.file_path);
    if file_path.exists() {
        std::fs::remove_file(&file_path).map_err(|e| format!("Failed to delete file: {}", e))?;

        // Try to clean up empty parent directories
        if let Some(parent) = file_path.parent() {
            let _ = std::fs::remove_dir(parent); // Ignore errors - dir may not be empty
            if let Some(grandparent) = parent.parent() {
                let _ = std::fs::remove_dir(grandparent); // Clean up conversation dir if empty
            }
        }
    }

    // Delete database record
    state
        .chat_attachment_repo
        .delete(&attachment_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(test)]
#[path = "chat_attachment_commands_tests.rs"]
mod tests;
