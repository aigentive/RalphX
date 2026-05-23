use super::*;
use crate::domain::entities::ChatConversationId;
use std::sync::Arc;

// Mock implementation for testing trait object usage
struct MockChatAttachmentRepository {
    attachments: Vec<ChatAttachment>,
}

impl MockChatAttachmentRepository {
    fn new() -> Self {
        Self {
            attachments: vec![],
        }
    }

    fn with_attachments(attachments: Vec<ChatAttachment>) -> Self {
        Self { attachments }
    }
}

#[async_trait]
impl ChatAttachmentRepository for MockChatAttachmentRepository {
    async fn create(&self, attachment: ChatAttachment) -> AppResult<ChatAttachment> {
        Ok(attachment)
    }

    async fn get_by_id(&self, id: &ChatAttachmentId) -> AppResult<Option<ChatAttachment>> {
        Ok(self.attachments.iter().find(|a| a.id == *id).cloned())
    }

    async fn find_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatAttachment>> {
        Ok(self
            .attachments
            .iter()
            .filter(|a| a.conversation_id == *conversation_id)
            .cloned()
            .collect())
    }

    async fn find_by_message_id(
        &self,
        message_id: &ChatMessageId,
    ) -> AppResult<Vec<ChatAttachment>> {
        Ok(self
            .attachments
            .iter()
            .filter(|a| a.message_id.as_ref() == Some(message_id))
            .cloned()
            .collect())
    }

    async fn update_message_id(
        &self,
        _id: &ChatAttachmentId,
        _message_id: &ChatMessageId,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_message_ids(
        &self,
        _attachment_ids: &[ChatAttachmentId],
        _message_id: &ChatMessageId,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn reparent_pending_attachments(
        &self,
        _from_conversation_id: &ChatConversationId,
        _to_conversation_id: &ChatConversationId,
    ) -> AppResult<usize> {
        Ok(0)
    }

    async fn delete(&self, _id: &ChatAttachmentId) -> AppResult<()> {
        Ok(())
    }

    async fn delete_by_conversation_id(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        Ok(())
    }
}

#[test]
fn test_trait_object_safety() {
    let repo = MockChatAttachmentRepository::new();
    let _: Arc<dyn ChatAttachmentRepository> = Arc::new(repo);
}

#[test]
fn test_mock_with_attachments() {
    let conversation_id = ChatConversationId::new();
    let attachment = ChatAttachment::new(
        conversation_id,
        "test.txt",
        "/path/to/test.txt",
        1024,
        Some("text/plain".to_string()),
    );
    let repo = MockChatAttachmentRepository::with_attachments(vec![attachment.clone()]);

    assert_eq!(repo.attachments.len(), 1);
    assert_eq!(repo.attachments[0].id, attachment.id);
}
