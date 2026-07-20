// ChatAttachmentService
// Application service for managing chat attachments: upload, link, list, delete

use std::path::PathBuf;
use std::sync::Arc;

use crate::domain::entities::{
    ChatAttachment, ChatAttachmentId, ChatConversationId, ChatMessageId,
};
use crate::domain::repositories::ChatAttachmentRepository;
use crate::error::{AppError, AppResult};

use super::chat_attachment_storage::build_chat_attachment_file_path;

/// Service for managing chat file attachments
pub struct ChatAttachmentService<R: ChatAttachmentRepository + ?Sized> {
    /// Repository for ChatAttachment records
    repository: Arc<R>,
    /// Base directory for attachment storage
    storage_base_path: PathBuf,
}

impl<R: ChatAttachmentRepository + ?Sized> ChatAttachmentService<R> {
    /// Create a new chat attachment service
    pub fn new(repository: Arc<R>, storage_base_path: impl Into<PathBuf>) -> Self {
        Self {
            repository,
            storage_base_path: storage_base_path.into(),
        }
    }

    /// Upload a file attachment and create a database record
    ///
    /// Storage path uses app-owned storage plus hash-derived conversation and attachment
    /// components. The original file name remains metadata, not a raw path component.
    ///
    /// Returns the created attachment with generated ID
    pub async fn upload(
        &self,
        conversation_id: &ChatConversationId,
        file_name: impl Into<String>,
        file_data: &[u8],
        mime_type: Option<String>,
    ) -> AppResult<ChatAttachment> {
        let file_name = file_name.into();
        let file_size = file_data.len() as i64;

        // Generate attachment ID and determine storage path
        let attachment_id = ChatAttachmentId::new();
        let file_path = build_chat_attachment_file_path(
            &self.storage_base_path,
            conversation_id,
            &attachment_id,
            &file_name,
        )
        .map_err(AppError::Validation)?;

        // Create parent directories
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::Infrastructure(format!("Failed to create directory: {}", e))
            })?;
        }

        // Write file to disk
        std::fs::write(&file_path, file_data)
            .map_err(|e| AppError::Infrastructure(format!("Failed to write file: {}", e)))?;

        // Create database record
        let attachment = ChatAttachment::new(
            *conversation_id,
            file_name,
            file_path.to_string_lossy().to_string(),
            file_size,
            mime_type,
        );

        self.repository.create(attachment).await
    }

    /// Link one or more attachments to a message (after message is sent)
    pub async fn link_to_message(
        &self,
        attachment_ids: &[ChatAttachmentId],
        message_id: &ChatMessageId,
    ) -> AppResult<()> {
        if attachment_ids.is_empty() {
            return Ok(());
        }

        self.repository
            .update_message_ids(attachment_ids, message_id)
            .await
    }

    /// List all attachments for a conversation
    pub async fn list_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatAttachment>> {
        self.repository
            .find_by_conversation_id(conversation_id)
            .await
    }

    /// List all attachments for a specific message
    pub async fn list_for_message(
        &self,
        message_id: &ChatMessageId,
    ) -> AppResult<Vec<ChatAttachment>> {
        self.repository.find_by_message_id(message_id).await
    }

    /// Delete an attachment (removes file and database record)
    pub async fn delete(&self, id: &ChatAttachmentId) -> AppResult<()> {
        // Get attachment to find file path
        let attachment = self
            .repository
            .get_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Attachment {} not found", id)))?;

        // Delete file from disk (ignore errors if file doesn't exist)
        let file_path = PathBuf::from(&attachment.file_path);
        if file_path.exists() {
            std::fs::remove_file(&file_path)
                .map_err(|e| AppError::Infrastructure(format!("Failed to delete file: {}", e)))?;

            // Try to clean up empty parent directories
            if let Some(parent) = file_path.parent() {
                let _ = std::fs::remove_dir(parent); // Ignore errors - dir may not be empty
                if let Some(grandparent) = parent.parent() {
                    let _ = std::fs::remove_dir(grandparent); // Clean up conversation dir if empty
                }
            }
        }

        // Delete database record
        self.repository.delete(id).await
    }
}

#[cfg(test)]
#[path = "chat_attachment_service_tests.rs"]
mod tests;
