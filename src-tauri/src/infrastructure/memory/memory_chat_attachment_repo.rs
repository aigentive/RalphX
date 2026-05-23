// In-memory ChatAttachmentRepository implementation for testing
// Uses RwLock<HashMap> for thread-safe in-memory storage

use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;

use crate::domain::entities::{
    ChatAttachment, ChatAttachmentId, ChatConversationId, ChatMessageId,
};
use crate::domain::repositories::ChatAttachmentRepository;
use crate::error::AppResult;

/// In-memory implementation of ChatAttachmentRepository for testing
pub struct MemoryChatAttachmentRepository {
    attachments: RwLock<HashMap<String, ChatAttachment>>,
}

impl MemoryChatAttachmentRepository {
    /// Create a new empty repository
    pub fn new() -> Self {
        Self {
            attachments: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryChatAttachmentRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatAttachmentRepository for MemoryChatAttachmentRepository {
    async fn create(&self, attachment: ChatAttachment) -> AppResult<ChatAttachment> {
        self.attachments
            .write()
            .unwrap()
            .insert(attachment.id.as_str(), attachment.clone());
        Ok(attachment)
    }

    async fn get_by_id(&self, id: &ChatAttachmentId) -> AppResult<Option<ChatAttachment>> {
        Ok(self.attachments.read().unwrap().get(&id.as_str()).cloned())
    }

    async fn find_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatAttachment>> {
        let mut attachments: Vec<_> = self
            .attachments
            .read()
            .unwrap()
            .values()
            .filter(|a| a.conversation_id == *conversation_id)
            .cloned()
            .collect();
        attachments.sort_by_key(|a| a.created_at);
        Ok(attachments)
    }

    async fn find_by_message_id(
        &self,
        message_id: &ChatMessageId,
    ) -> AppResult<Vec<ChatAttachment>> {
        let mut attachments: Vec<_> = self
            .attachments
            .read()
            .unwrap()
            .values()
            .filter(|a| a.message_id.as_ref() == Some(message_id))
            .cloned()
            .collect();
        attachments.sort_by_key(|a| a.created_at);
        Ok(attachments)
    }

    async fn update_message_id(
        &self,
        id: &ChatAttachmentId,
        message_id: &ChatMessageId,
    ) -> AppResult<()> {
        if let Some(attachment) = self.attachments.write().unwrap().get_mut(&id.as_str()) {
            attachment.set_message_id(message_id.clone());
        }
        Ok(())
    }

    async fn update_message_ids(
        &self,
        attachment_ids: &[ChatAttachmentId],
        message_id: &ChatMessageId,
    ) -> AppResult<()> {
        for id in attachment_ids {
            self.update_message_id(id, message_id).await?;
        }
        Ok(())
    }

    async fn reparent_pending_attachments(
        &self,
        from_conversation_id: &ChatConversationId,
        to_conversation_id: &ChatConversationId,
    ) -> AppResult<usize> {
        let mut attachments = self.attachments.write().unwrap();
        let mut count = 0;
        for attachment in attachments.values_mut() {
            if attachment.conversation_id == *from_conversation_id && attachment.message_id.is_none()
            {
                attachment.conversation_id = to_conversation_id.clone();
                count += 1;
            }
        }
        Ok(count)
    }

    async fn delete(&self, id: &ChatAttachmentId) -> AppResult<()> {
        self.attachments.write().unwrap().remove(&id.as_str());
        Ok(())
    }

    async fn delete_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        self.attachments
            .write()
            .unwrap()
            .retain(|_, a| a.conversation_id != *conversation_id);
        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_chat_attachment_repo_tests.rs"]
mod tests;
