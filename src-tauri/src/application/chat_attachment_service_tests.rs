use super::*;
use crate::domain::repositories::ChatAttachmentRepository;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::Mutex;

// Mock repository for testing
struct MockChatAttachmentRepository {
    attachments: Arc<Mutex<HashMap<String, ChatAttachment>>>,
}

impl MockChatAttachmentRepository {
    fn new() -> Self {
        Self {
            attachments: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ChatAttachmentRepository for MockChatAttachmentRepository {
    async fn create(&self, attachment: ChatAttachment) -> AppResult<ChatAttachment> {
        let mut map = self.attachments.lock().await;
        map.insert(attachment.id.as_str().to_string(), attachment.clone());
        Ok(attachment)
    }

    async fn get_by_id(&self, id: &ChatAttachmentId) -> AppResult<Option<ChatAttachment>> {
        let map = self.attachments.lock().await;
        Ok(map.get(&id.as_str().to_string()).cloned())
    }

    async fn find_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatAttachment>> {
        let map = self.attachments.lock().await;
        Ok(map
            .values()
            .filter(|a| a.conversation_id == *conversation_id)
            .cloned()
            .collect())
    }

    async fn find_by_message_id(
        &self,
        message_id: &ChatMessageId,
    ) -> AppResult<Vec<ChatAttachment>> {
        let map = self.attachments.lock().await;
        Ok(map
            .values()
            .filter(|a| a.message_id.as_ref() == Some(message_id))
            .cloned()
            .collect())
    }

    async fn update_message_id(
        &self,
        id: &ChatAttachmentId,
        message_id: &ChatMessageId,
    ) -> AppResult<()> {
        let mut map = self.attachments.lock().await;
        if let Some(attachment) = map.get_mut(&id.as_str().to_string()) {
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
        let mut map = self.attachments.lock().await;
        let mut count = 0;
        for attachment in map.values_mut() {
            if attachment.conversation_id == *from_conversation_id
                && attachment.message_id.is_none()
            {
                attachment.conversation_id = to_conversation_id.clone();
                count += 1;
            }
        }
        Ok(count)
    }

    async fn delete(&self, id: &ChatAttachmentId) -> AppResult<()> {
        let mut map = self.attachments.lock().await;
        map.remove(&id.as_str().to_string());
        Ok(())
    }

    async fn delete_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        let mut map = self.attachments.lock().await;
        map.retain(|_, a| a.conversation_id != *conversation_id);
        Ok(())
    }
}

#[tokio::test]
async fn test_upload_creates_file_and_db_record() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = Arc::new(MockChatAttachmentRepository::new());
    let service = ChatAttachmentService::new(repo.clone(), temp_dir.path());

    let conversation_id = ChatConversationId::new();
    let file_data = b"Hello, world!";

    let result = service
        .upload(
            &conversation_id,
            "test.txt",
            file_data,
            Some("text/plain".to_string()),
        )
        .await;

    assert!(result.is_ok());
    let attachment = result.unwrap();
    assert_eq!(attachment.file_name, "test.txt");
    assert_eq!(attachment.file_size, 13);
    assert_eq!(attachment.conversation_id, conversation_id);

    // Verify file exists
    let file_path = PathBuf::from(&attachment.file_path);
    assert!(file_path.exists());
    let content = std::fs::read(&file_path).unwrap();
    assert_eq!(content, file_data);

    // Verify DB record
    let stored = repo.get_by_id(&attachment.id).await.unwrap();
    assert!(stored.is_some());
}

#[tokio::test]
async fn test_link_to_message() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = Arc::new(MockChatAttachmentRepository::new());
    let service = ChatAttachmentService::new(repo.clone(), temp_dir.path());

    let conversation_id = ChatConversationId::new();
    let attachment = service
        .upload(&conversation_id, "file.txt", b"data", None)
        .await
        .unwrap();

    let message_id = ChatMessageId::new();
    let result = service.link_to_message(&[attachment.id], &message_id).await;

    assert!(result.is_ok());

    // Verify attachment is linked
    let updated = repo.get_by_id(&attachment.id).await.unwrap().unwrap();
    assert_eq!(updated.message_id, Some(message_id));
}

#[tokio::test]
async fn test_list_for_conversation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = Arc::new(MockChatAttachmentRepository::new());
    let service = ChatAttachmentService::new(repo.clone(), temp_dir.path());

    let conversation_id = ChatConversationId::new();
    service
        .upload(&conversation_id, "file1.txt", b"data1", None)
        .await
        .unwrap();
    service
        .upload(&conversation_id, "file2.txt", b"data2", None)
        .await
        .unwrap();

    let attachments = service
        .list_for_conversation(&conversation_id)
        .await
        .unwrap();
    assert_eq!(attachments.len(), 2);
}

#[tokio::test]
async fn test_delete_removes_file_and_record() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repo = Arc::new(MockChatAttachmentRepository::new());
    let service = ChatAttachmentService::new(repo.clone(), temp_dir.path());

    let conversation_id = ChatConversationId::new();
    let attachment = service
        .upload(&conversation_id, "file.txt", b"data", None)
        .await
        .unwrap();

    let file_path = PathBuf::from(&attachment.file_path);
    assert!(file_path.exists());

    let result = service.delete(&attachment.id).await;
    assert!(result.is_ok());

    // Verify file is deleted
    assert!(!file_path.exists());

    // Verify DB record is deleted
    let stored = repo.get_by_id(&attachment.id).await.unwrap();
    assert!(stored.is_none());
}
