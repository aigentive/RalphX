use super::sqlite_chat_conversation_repo::SqliteChatConversationRepository;
use super::sqlite_chat_timeline_repo::SqliteChatTimelineRepository;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    ChatConversation, ChatMessageId, ChatTimelineItem, ChatTimelineItemKind,
    ChatTimelineItemStatus, MessageRole, ProjectId,
};
use crate::domain::repositories::{ChatConversationRepository, ChatTimelineRepository};
use crate::testing::SqliteTestDb;

fn setup_repos() -> (
    SqliteTestDb,
    SqliteChatConversationRepository,
    SqliteChatTimelineRepository,
) {
    let db = SqliteTestDb::new("sqlite-chat-timeline-repo");
    let conversation_repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let timeline_repo = SqliteChatTimelineRepository::from_shared(db.shared_conn());
    (db, conversation_repo, timeline_repo)
}

async fn create_conversation(
    repo: &SqliteChatConversationRepository,
) -> crate::domain::entities::ChatConversationId {
    repo.create(ChatConversation::new_project(ProjectId::new()))
        .await
        .expect("create conversation")
        .id
}

fn insert_parent_message(
    db: &SqliteTestDb,
    conversation_id: crate::domain::entities::ChatConversationId,
    message_id: &ChatMessageId,
) {
    db.with_connection(|conn| {
        conn.execute(
            r#"
            INSERT INTO chat_messages (id, conversation_id, role, content, created_at)
            VALUES (?1, ?2, 'orchestrator', '', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
            "#,
            rusqlite::params![message_id.as_str(), conversation_id.as_str()],
        )
        .expect("insert parent chat message");
    });
}

fn text_item(
    conversation_id: crate::domain::entities::ChatConversationId,
    message_id: &ChatMessageId,
    block_index: i64,
    text: &str,
) -> ChatTimelineItem {
    let mut item = ChatTimelineItem::for_message_block(
        message_id.clone(),
        conversation_id,
        block_index,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::Text,
    );
    item.status = ChatTimelineItemStatus::Streaming;
    item.text = Some(text.to_string());
    item
}

#[tokio::test]
async fn upsert_assigns_sequences_and_preserves_existing_sequence_on_update() {
    let (db, conversation_repo, timeline_repo) = setup_repos();
    let conversation_id = create_conversation(&conversation_repo).await;
    let message_id = ChatMessageId::from_string("assistant-message-1");
    insert_parent_message(&db, conversation_id, &message_id);

    let first = timeline_repo
        .upsert_item(text_item(conversation_id, &message_id, 0, "first"))
        .await
        .expect("insert first item");
    assert_eq!(first.sequence, 1);

    let mut tool = ChatTimelineItem::for_message_block(
        message_id.clone(),
        conversation_id,
        1,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    tool.tool_call_id = Some("tool-1".to_string());
    tool.tool_name = Some("bash".to_string());
    tool.tool_status = Some("pending".to_string());
    tool.tool_input_preview = Some(r#"{"command":"cargo test"}"#.to_string());
    tool.input_json = Some(r#"{"command":"cargo test"}"#.to_string());
    tool.raw_block_json = Some(r#"{"type":"tool_use","id":"tool-1"}"#.to_string());
    tool.provider_harness = Some(AgentHarnessKind::Codex);
    tool.provider_session_id = Some("thread-1".to_string());

    let inserted_tool = timeline_repo
        .upsert_item(tool.clone())
        .await
        .expect("insert tool item");
    assert_eq!(inserted_tool.sequence, 2);

    tool.status = ChatTimelineItemStatus::Finalized;
    tool.tool_status = Some("completed".to_string());
    tool.result_json = Some(r#""ok""#.to_string());
    tool.tool_result_preview = Some(r#""ok""#.to_string());
    let updated_tool = timeline_repo
        .upsert_item(tool.clone())
        .await
        .expect("update tool item");

    assert_eq!(updated_tool.sequence, 2);
    assert_eq!(
        timeline_repo
            .count_by_conversation(&conversation_id)
            .await
            .expect("count items"),
        2
    );

    let loaded = timeline_repo
        .get_by_id(&tool.id)
        .await
        .expect("load item")
        .expect("item exists");
    assert_eq!(loaded.status, ChatTimelineItemStatus::Finalized);
    assert_eq!(loaded.result_json.as_deref(), Some(r#""ok""#));
    assert_eq!(loaded.provider_harness, Some(AgentHarnessKind::Codex));
    assert_eq!(loaded.provider_session_id.as_deref(), Some("thread-1"));
}

#[tokio::test]
async fn page_returns_visible_tail_and_older_cursor_without_eager_payloads() {
    let (db, conversation_repo, timeline_repo) = setup_repos();
    let conversation_id = create_conversation(&conversation_repo).await;
    let message_id = ChatMessageId::from_string("assistant-message-2");
    insert_parent_message(&db, conversation_id, &message_id);

    for index in 0..3 {
        let mut item = text_item(
            conversation_id,
            &message_id,
            index,
            &format!("block {index}"),
        );
        item.raw_block_json = Some(format!(r#"{{"text":"block {index}"}}"#));
        timeline_repo
            .upsert_item(item)
            .await
            .expect("insert timeline item");
    }

    let newest = timeline_repo
        .get_page(&conversation_id, 2, None)
        .await
        .expect("newest page");
    assert_eq!(newest.items.len(), 2);
    assert_eq!(newest.total_item_count, 3);
    assert!(newest.has_older);
    assert_eq!(newest.oldest_loaded_sequence, Some(2));
    assert_eq!(newest.newest_loaded_sequence, Some(3));
    assert_eq!(
        newest
            .items
            .iter()
            .map(|item| item.text.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("block 1"), Some("block 2")]
    );
    assert!(newest
        .items
        .iter()
        .all(|item| item.raw_block_json.is_none()));

    let older = timeline_repo
        .get_page(&conversation_id, 2, newest.oldest_loaded_sequence)
        .await
        .expect("older page");
    assert_eq!(older.items.len(), 1);
    assert!(!older.has_older);
    assert_eq!(older.items[0].text.as_deref(), Some("block 0"));
}

#[tokio::test]
async fn mark_message_items_finalized_updates_streaming_rows() {
    let (db, conversation_repo, timeline_repo) = setup_repos();
    let conversation_id = create_conversation(&conversation_repo).await;
    let message_id = ChatMessageId::from_string("assistant-message-3");
    insert_parent_message(&db, conversation_id, &message_id);
    let item = timeline_repo
        .upsert_item(text_item(conversation_id, &message_id, 0, "streaming"))
        .await
        .expect("insert streaming item");

    timeline_repo
        .mark_message_items_finalized(&message_id)
        .await
        .expect("finalize message items");

    let loaded = timeline_repo
        .get_by_id(&item.id)
        .await
        .expect("load item")
        .expect("item exists");
    assert_eq!(loaded.status, ChatTimelineItemStatus::Finalized);
    assert!(loaded.finalized_at.is_some());
}
