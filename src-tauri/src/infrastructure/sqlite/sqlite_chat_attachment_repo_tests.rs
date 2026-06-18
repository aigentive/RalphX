// Tests for SqliteChatAttachmentRepository

use super::sqlite_chat_attachment_repo::SqliteChatAttachmentRepository;
use crate::domain::entities::{
    ChatAttachment, ChatAttachmentId, ChatConversationId, ChatMessageId,
};
use crate::domain::repositories::ChatAttachmentRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_chat_attachment_repo_tests")
}

fn create_test_conversation(db: &SqliteTestDb) -> ChatConversationId {
    let id = ChatConversationId::new();
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (id, context_type, context_id, created_at, updated_at)
             VALUES (?1, 'project', 'test-project', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            [id.as_str()],
        )
        .unwrap();
    });
    id
}

// ==================== CREATE TESTS ====================

#[tokio::test]
async fn test_create_attachment_without_message_id() {
    let db = setup_test_db();
    let conversation_id = create_test_conversation(&db);

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let attachment = ChatAttachment::new(
        conversation_id,
        "test.txt",
        "/path/to/test.txt",
        1024,
        Some("text/plain".to_string()),
    );

    let result = repo.create(attachment.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id, attachment.id);
    assert_eq!(created.file_name, "test.txt");
    assert_eq!(created.file_path, "/path/to/test.txt");
    assert_eq!(created.file_size, 1024);
    assert_eq!(created.mime_type, Some("text/plain".to_string()));
    assert_eq!(created.message_id, None);
}

#[tokio::test]
async fn test_create_attachment_with_message_id() {
    let db = setup_test_db();
    let conversation_id = create_test_conversation(&db);
    let message_id = ChatMessageId::new();

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let mut attachment = ChatAttachment::new(
        conversation_id,
        "document.pdf",
        "/path/to/document.pdf",
        2048,
        Some("application/pdf".to_string()),
    );
    attachment.set_message_id(message_id.clone());

    let result = repo.create(attachment.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.message_id, Some(message_id));
}

#[tokio::test]
async fn test_create_attachment_without_mime_type() {
    let db = setup_test_db();
    let conversation_id = create_test_conversation(&db);

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let attachment = ChatAttachment::new(conversation_id, "README", "/path/to/README", 512, None);

    let result = repo.create(attachment.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.mime_type, None);
}

// ==================== GET_BY_ID TESTS ====================

#[tokio::test]
async fn test_get_by_id_returns_attachment() {
    let db = setup_test_db();
    let conversation_id = create_test_conversation(&db);

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let attachment = ChatAttachment::new(
        conversation_id,
        "image.png",
        "/path/to/image.png",
        4096,
        Some("image/png".to_string()),
    );
    repo.create(attachment.clone()).await.unwrap();

    let result = repo.get_by_id(&attachment.id).await;

    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.id, attachment.id);
    assert_eq!(found.file_name, "image.png");
}

#[tokio::test]
async fn test_get_by_id_not_found_returns_none() {
    let db = setup_test_db();
    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let non_existent_id = ChatAttachmentId::new();

    let result = repo.get_by_id(&non_existent_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

// ==================== FIND_BY_CONVERSATION_ID TESTS ====================

#[tokio::test]
async fn test_find_by_conversation_id_returns_attachments() {
    let db = setup_test_db();
    let conversation_id = create_test_conversation(&db);

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let attachment1 = ChatAttachment::new(
        conversation_id,
        "file1.txt",
        "/path/to/file1.txt",
        1024,
        None,
    );
    let attachment2 = ChatAttachment::new(
        conversation_id,
        "file2.txt",
        "/path/to/file2.txt",
        2048,
        None,
    );
    repo.create(attachment1.clone()).await.unwrap();
    repo.create(attachment2.clone()).await.unwrap();

    let result = repo.find_by_conversation_id(&conversation_id).await;

    assert!(result.is_ok());
    let attachments = result.unwrap();
    assert_eq!(attachments.len(), 2);
    assert_eq!(attachments[0].file_name, "file1.txt");
    assert_eq!(attachments[1].file_name, "file2.txt");
}

#[tokio::test]
async fn test_find_by_conversation_id_empty_list() {
    let db = setup_test_db();
    let conversation_id = create_test_conversation(&db);

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let result = repo.find_by_conversation_id(&conversation_id).await;

    assert!(result.is_ok());
    let attachments = result.unwrap();
    assert_eq!(attachments.len(), 0);
}

// ==================== FIND_BY_MESSAGE_ID TESTS ====================

#[tokio::test]
async fn test_find_by_message_id_returns_attachments() {
    let db = setup_test_db();
    let conversation_id = create_test_conversation(&db);
    let message_id = ChatMessageId::new();

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let mut attachment1 = ChatAttachment::new(
        conversation_id,
        "file1.txt",
        "/path/to/file1.txt",
        1024,
        None,
    );
    attachment1.set_message_id(message_id.clone());
    let mut attachment2 = ChatAttachment::new(
        conversation_id,
        "file2.txt",
        "/path/to/file2.txt",
        2048,
        None,
    );
    attachment2.set_message_id(message_id.clone());

    repo.create(attachment1.clone()).await.unwrap();
    repo.create(attachment2.clone()).await.unwrap();

    let result = repo.find_by_message_id(&message_id).await;

    assert!(result.is_ok());
    let attachments = result.unwrap();
    assert_eq!(attachments.len(), 2);
}

#[tokio::test]
async fn test_find_by_message_id_empty_list() {
    let db = setup_test_db();
    let non_existent_message_id = ChatMessageId::new();

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let result = repo.find_by_message_id(&non_existent_message_id).await;

    assert!(result.is_ok());
    let attachments = result.unwrap();
    assert_eq!(attachments.len(), 0);
}

// ==================== UPDATE_MESSAGE_ID TESTS ====================

#[tokio::test]
async fn test_update_message_id_links_attachment_to_message() {
    let db = setup_test_db();
    let conversation_id = create_test_conversation(&db);

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let attachment =
        ChatAttachment::new(conversation_id, "file.txt", "/path/to/file.txt", 1024, None);
    repo.create(attachment.clone()).await.unwrap();

    let message_id = ChatMessageId::new();
    let result = repo.update_message_id(&attachment.id, &message_id).await;

    assert!(result.is_ok());

    let updated = repo.get_by_id(&attachment.id).await.unwrap().unwrap();
    assert_eq!(updated.message_id, Some(message_id));
}

// ==================== UPDATE_MESSAGE_IDS TESTS ====================

#[tokio::test]
async fn test_update_message_ids_links_multiple_attachments() {
    let db = setup_test_db();
    let conversation_id = create_test_conversation(&db);

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let attachment1 = ChatAttachment::new(
        conversation_id,
        "file1.txt",
        "/path/to/file1.txt",
        1024,
        None,
    );
    let attachment2 = ChatAttachment::new(
        conversation_id,
        "file2.txt",
        "/path/to/file2.txt",
        2048,
        None,
    );
    repo.create(attachment1.clone()).await.unwrap();
    repo.create(attachment2.clone()).await.unwrap();

    let message_id = ChatMessageId::new();
    let attachment_ids = vec![attachment1.id, attachment2.id];
    let result = repo.update_message_ids(&attachment_ids, &message_id).await;

    assert!(result.is_ok());

    let updated1 = repo.get_by_id(&attachment1.id).await.unwrap().unwrap();
    let updated2 = repo.get_by_id(&attachment2.id).await.unwrap().unwrap();
    assert_eq!(updated1.message_id, Some(message_id.clone()));
    assert_eq!(updated2.message_id, Some(message_id));
}

// ==================== DELETE TESTS ====================

#[tokio::test]
async fn test_delete_removes_attachment() {
    let db = setup_test_db();
    let conversation_id = create_test_conversation(&db);

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let attachment =
        ChatAttachment::new(conversation_id, "file.txt", "/path/to/file.txt", 1024, None);
    repo.create(attachment.clone()).await.unwrap();

    let result = repo.delete(&attachment.id).await;

    assert!(result.is_ok());
    let found = repo.get_by_id(&attachment.id).await.unwrap();
    assert!(found.is_none());
}

// ==================== DELETE_BY_CONVERSATION_ID TESTS ====================

#[tokio::test]
async fn test_delete_by_conversation_id_removes_all_attachments() {
    let db = setup_test_db();
    let conversation_id = create_test_conversation(&db);

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let attachment1 = ChatAttachment::new(
        conversation_id,
        "file1.txt",
        "/path/to/file1.txt",
        1024,
        None,
    );
    let attachment2 = ChatAttachment::new(
        conversation_id,
        "file2.txt",
        "/path/to/file2.txt",
        2048,
        None,
    );
    repo.create(attachment1.clone()).await.unwrap();
    repo.create(attachment2.clone()).await.unwrap();

    let result = repo.delete_by_conversation_id(&conversation_id).await;

    assert!(result.is_ok());
    let attachments = repo
        .find_by_conversation_id(&conversation_id)
        .await
        .unwrap();
    assert_eq!(attachments.len(), 0);
}

// Note: Cascade delete on conversation deletion is tested in migration tests
// See v34_chat_attachments_tests::test_v34_foreign_key_cascade_delete

// ==================== REPARENT_PENDING_ATTACHMENTS TESTS ====================

#[tokio::test]
async fn test_reparent_pending_attachments_moves_only_unsent() {
    let db = setup_test_db();
    let from_conversation = create_test_conversation(&db);
    let to_conversation = create_test_conversation(&db);

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());

    let pending = ChatAttachment::new(
        from_conversation,
        "pending.txt",
        "/path/to/pending.txt",
        100,
        None,
    );
    let mut sent = ChatAttachment::new(
        from_conversation,
        "sent.txt",
        "/path/to/sent.txt",
        200,
        None,
    );
    sent.set_message_id(ChatMessageId::new());

    let pending_id = pending.id;
    let sent_id = sent.id;
    repo.create(pending).await.unwrap();
    repo.create(sent).await.unwrap();

    let count = repo
        .reparent_pending_attachments(&from_conversation, &to_conversation)
        .await
        .unwrap();
    assert_eq!(count, 1);

    let moved = repo.get_by_id(&pending_id).await.unwrap().unwrap();
    assert_eq!(moved.conversation_id, to_conversation);

    let stayed = repo.get_by_id(&sent_id).await.unwrap().unwrap();
    assert_eq!(stayed.conversation_id, from_conversation);
}

#[tokio::test]
async fn test_reparent_pending_attachments_returns_zero_when_none_pending() {
    let db = setup_test_db();
    let from_conversation = create_test_conversation(&db);
    let to_conversation = create_test_conversation(&db);

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());

    let mut sent = ChatAttachment::new(
        from_conversation,
        "sent.txt",
        "/path/to/sent.txt",
        200,
        None,
    );
    sent.set_message_id(ChatMessageId::new());
    repo.create(sent).await.unwrap();

    let count = repo
        .reparent_pending_attachments(&from_conversation, &to_conversation)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

// ==================== UPDATE_MESSAGE_IDS EDGE CASE TESTS ====================

#[tokio::test]
async fn test_update_message_ids_with_empty_slice_is_noop() {
    let db = setup_test_db();
    let conversation_id = create_test_conversation(&db);

    let repo = SqliteChatAttachmentRepository::from_shared(db.shared_conn());
    let attachment =
        ChatAttachment::new(conversation_id, "file.txt", "/path/to/file.txt", 1024, None);
    repo.create(attachment.clone()).await.unwrap();

    let message_id = ChatMessageId::new();
    // Calling with empty slice should not error and should not update anything
    let result = repo.update_message_ids(&[], &message_id).await;
    assert!(result.is_ok());

    // The attachment should remain unchanged (message_id still None)
    let found = repo.get_by_id(&attachment.id).await.unwrap().unwrap();
    assert!(found.message_id.is_none());
}
