// Chat attachment repository trait - domain layer abstraction
//
// This trait defines the contract for chat attachment persistence.
// Attachments are files uploaded to conversations and optionally linked to messages.

use async_trait::async_trait;

use crate::domain::entities::{
    ChatAttachment, ChatAttachmentId, ChatConversationId, ChatMessageId,
};
use crate::error::AppResult;

/// Repository trait for ChatAttachment persistence.
/// Implementations can use SQLite, PostgreSQL, in-memory, etc.
#[async_trait]
pub trait ChatAttachmentRepository: Send + Sync {
    /// Create a new attachment
    async fn create(&self, attachment: ChatAttachment) -> AppResult<ChatAttachment>;

    /// Get attachment by ID
    async fn get_by_id(&self, id: &ChatAttachmentId) -> AppResult<Option<ChatAttachment>>;

    /// Get all attachments for a conversation
    async fn find_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatAttachment>>;

    /// Get all attachments for a specific message
    async fn find_by_message_id(
        &self,
        message_id: &ChatMessageId,
    ) -> AppResult<Vec<ChatAttachment>>;

    /// Update the message_id for an attachment (link it to a message after send)
    async fn update_message_id(
        &self,
        id: &ChatAttachmentId,
        message_id: &ChatMessageId,
    ) -> AppResult<()>;

    /// Update the message_id for multiple attachments (batch link after send)
    async fn update_message_ids(
        &self,
        attachment_ids: &[ChatAttachmentId],
        message_id: &ChatMessageId,
    ) -> AppResult<()>;

    /// Delete an attachment
    async fn delete(&self, id: &ChatAttachmentId) -> AppResult<()>;

    /// Move pending (unsent) attachments from one conversation to another.
    /// Only attachments with message_id = NULL are moved.
    /// Returns the number of attachments re-parented.
    async fn reparent_pending_attachments(
        &self,
        from_conversation_id: &ChatConversationId,
        to_conversation_id: &ChatConversationId,
    ) -> AppResult<usize>;

    /// Delete all attachments for a conversation (called when conversation is deleted)
    async fn delete_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()>;
}

#[cfg(test)]
#[path = "chat_attachment_repository_tests.rs"]
mod tests;
