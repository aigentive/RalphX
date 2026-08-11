use super::sqlite_chat_conversation_repo::SqliteChatConversationRepository;
use super::sqlite_chat_timeline_repo::SqliteChatTimelineRepository;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    ChatConversation, ChatMessageId, ChatTimelineItem, ChatTimelineItemKind,
    ChatTimelineItemStatus, MessageRole, ProjectId,
};
use crate::domain::repositories::{ChatConversationRepository, ChatTimelineRepository};
use crate::testing::SqliteTestDb;
use serde_json::json;

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
async fn upsert_persists_raw_payload_only_for_full_fidelity_tools() {
    let (db, conversation_repo, timeline_repo) = setup_repos();
    let conversation_id = create_conversation(&conversation_repo).await;
    let message_id = ChatMessageId::from_string("assistant-message-raw-policy");
    insert_parent_message(&db, conversation_id, &message_id);

    let mut bash = ChatTimelineItem::for_message_block(
        message_id.clone(),
        conversation_id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    bash.tool_name = Some("bash".to_string());
    bash.input_json = Some(r#"{"command":"cargo test"}"#.to_string());
    bash.result_json = Some(r#""ok""#.to_string());
    bash.raw_block_json = Some(r#"{"type":"tool_use","name":"bash"}"#.to_string());
    let bash = timeline_repo.upsert_item(bash).await.expect("insert bash");

    let mut edit = ChatTimelineItem::for_message_block(
        message_id,
        conversation_id,
        1,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    edit.tool_name = Some("edit".to_string());
    edit.input_json = Some(r#"{"file_path":"src/lib.rs"}"#.to_string());
    edit.raw_block_json = Some(
        r#"{"type":"tool_use","name":"edit","diff_context":{"file_path":"src/lib.rs"}}"#
            .to_string(),
    );
    let edit = timeline_repo.upsert_item(edit).await.expect("insert edit");

    db.with_connection(|conn| {
        let bash_payload: (Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT input_json, result_json, raw_block_json FROM chat_message_block_payloads WHERE block_id = ?1",
                [bash.id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("bash payload");
        assert_eq!(bash_payload.0.as_deref(), Some(r#"{"command":"cargo test"}"#));
        assert_eq!(bash_payload.1.as_deref(), Some(r#""ok""#));
        assert!(bash_payload.2.is_none());

        let edit_raw: Option<String> = conn
            .query_row(
                "SELECT raw_block_json FROM chat_message_block_payloads WHERE block_id = ?1",
                [edit.id.as_str()],
                |row| row.get(0),
            )
            .expect("edit payload");
        assert!(edit_raw.is_some());
    });
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
async fn page_hydrates_ask_user_question_result_payloads() {
    let (db, conversation_repo, timeline_repo) = setup_repos();
    let conversation_id = create_conversation(&conversation_repo).await;
    let message_id = ChatMessageId::from_string("assistant-message-question");
    insert_parent_message(&db, conversation_id, &message_id);

    let answer_payload = json!({
        "content": [{
            "type": "text",
            "text": json!({
                "answers": [{
                    "id": "scope",
                    "request_id": "req-1",
                    "question": "Which area should we focus on?",
                    "options": [{ "label": "Backend", "value": "backend" }],
                    "selected_options": ["backend"],
                    "text": null,
                    "skipped": false
                }]
            }).to_string()
        }],
        "structured_content": null
    });

    let expected_result_json = answer_payload.to_string();
    let expected_raw_block_json =
        r#"{"type":"tool_use","id":"tool-ask-question","name":"mcp__ralphx__ask_user_question"}"#;

    let mut item = ChatTimelineItem::for_message_block(
        message_id.clone(),
        conversation_id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    item.status = ChatTimelineItemStatus::Finalized;
    item.tool_call_id = Some("tool-ask-question".to_string());
    item.tool_name = Some("mcp__ralphx__ask_user_question".to_string());
    item.tool_status = Some("completed".to_string());
    item.tool_input_preview = Some(r#"{"question":"Which area should we focus on?"}"#.to_string());
    item.input_json = item.tool_input_preview.clone();
    item.tool_result_preview = Some("preview-only answer payload".to_string());
    item.result_json = Some(expected_result_json.clone());
    item.raw_block_json = Some(expected_raw_block_json.to_string());

    timeline_repo
        .upsert_item(item)
        .await
        .expect("insert ask-user-question timeline item");

    let page = timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("timeline page");
    let hydrated = page.items.first().expect("timeline item");

    assert_eq!(
        hydrated.result_json.as_deref(),
        Some(expected_result_json.as_str())
    );
    assert_eq!(
        hydrated.raw_block_json.as_deref(),
        Some(expected_raw_block_json)
    );
}

#[tokio::test]
async fn page_hydrates_diff_tool_payloads_without_eager_loading_other_tools() {
    let (db, conversation_repo, timeline_repo) = setup_repos();
    let conversation_id = create_conversation(&conversation_repo).await;
    let message_id = ChatMessageId::from_string("assistant-message-diff-tool");
    insert_parent_message(&db, conversation_id, &message_id);

    let mut bash = ChatTimelineItem::for_message_block(
        message_id.clone(),
        conversation_id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    bash.tool_call_id = Some("tool-bash".to_string());
    bash.tool_name = Some("bash".to_string());
    bash.tool_status = Some("completed".to_string());
    bash.tool_input_preview = Some(r#"{"command":"cargo test"}"#.to_string());
    bash.input_json = Some(r#"{"command":"cargo test"}"#.to_string());
    bash.raw_block_json = Some(r#"{"type":"tool_use","id":"tool-bash"}"#.to_string());
    timeline_repo
        .upsert_item(bash)
        .await
        .expect("insert bash item");

    let edit_args = json!({
        "file_path": "src/lib.rs",
        "old_string": "old",
        "new_string": "new"
    });
    let mut edit = ChatTimelineItem::for_message_block(
        message_id,
        conversation_id,
        1,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    edit.tool_call_id = Some("tool-edit".to_string());
    edit.tool_name = Some("edit".to_string());
    edit.tool_status = Some("completed".to_string());
    edit.tool_input_preview = Some(r#"{"file_path":"src/lib.rs","old_string":"old""#.to_string());
    edit.input_json = Some(edit_args.to_string());
    edit.raw_block_json = Some(
        json!({
            "type": "tool_use",
            "id": "tool-edit",
            "name": "edit",
            "arguments": edit_args,
            "diff_context": {
                "file_path": "src/lib.rs",
                "old_content": "old"
            }
        })
        .to_string(),
    );
    timeline_repo
        .upsert_item(edit)
        .await
        .expect("insert edit item");

    let page = timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("timeline page");

    let bash = page
        .items
        .iter()
        .find(|item| item.tool_name.as_deref() == Some("bash"))
        .expect("bash row");
    assert!(bash.input_json.is_none());
    assert!(bash.raw_block_json.is_none());

    let edit = page
        .items
        .iter()
        .find(|item| item.tool_name.as_deref() == Some("edit"))
        .expect("edit row");
    let hydrated_args: serde_json::Value =
        serde_json::from_str(edit.input_json.as_deref().expect("edit input_json"))
            .expect("edit input json should parse");
    assert_eq!(hydrated_args["file_path"], "src/lib.rs");
    assert_eq!(hydrated_args["old_string"], "old");
    assert_eq!(hydrated_args["new_string"], "new");
    assert!(edit
        .raw_block_json
        .as_deref()
        .expect("edit raw block json")
        .contains("diff_context"));
}

#[tokio::test]
async fn page_hydrates_full_delegation_payloads_without_eager_loading_other_tools() {
    let (db, conversation_repo, timeline_repo) = setup_repos();
    let conversation_id = create_conversation(&conversation_repo).await;
    let message_id = ChatMessageId::from_string("assistant-message-delegation");
    insert_parent_message(&db, conversation_id, &message_id);

    let long_result = json!({
        "job_id": "job-full-payload",
        "status": "completed",
        "content": "x".repeat(2_000),
        "delegated_status": {
            "conversation_id": "delegated-conversation",
            "latest_run": {
                "agent_run_id": "delegated-run",
                "logical_model": "gpt-5.4",
                "logical_effort": "high"
            }
        }
    });
    let mut delegate = ChatTimelineItem::for_message_block(
        message_id,
        conversation_id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    delegate.tool_call_id = Some("call-delegate-wait".to_string());
    delegate.tool_name = Some("mcp__ralphx__delegate_wait".to_string());
    delegate.tool_status = Some("completed".to_string());
    delegate.tool_input_preview = Some(r#"{"job_id":"job-full-payload"}"#.to_string());
    delegate.input_json = delegate.tool_input_preview.clone();
    delegate.tool_result_preview = Some("truncated-preview".to_string());
    delegate.result_json = Some(long_result.to_string());

    timeline_repo
        .upsert_item(delegate)
        .await
        .expect("insert delegation timeline item");

    let page = timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("timeline page");
    let hydrated = page.items.first().expect("delegation item");
    let hydrated_result: serde_json::Value = serde_json::from_str(
        hydrated
            .result_json
            .as_deref()
            .expect("delegation result should be fully hydrated"),
    )
    .expect("delegation result json");

    assert_eq!(hydrated_result["job_id"], "job-full-payload");
    assert_eq!(
        hydrated_result["delegated_status"]["latest_run"]["agent_run_id"],
        "delegated-run"
    );
    assert_eq!(
        hydrated_result["content"].as_str().map(str::len),
        Some(2_000)
    );
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

#[tokio::test]
async fn delete_message_items_except_block_indices_removes_obsolete_rows() {
    let (db, conversation_repo, timeline_repo) = setup_repos();
    let conversation_id = create_conversation(&conversation_repo).await;
    let message_id = ChatMessageId::from_string("assistant-message-delete-stale");
    insert_parent_message(&db, conversation_id, &message_id);
    timeline_repo
        .upsert_item(text_item(conversation_id, &message_id, 0, "keep"))
        .await
        .expect("insert kept item");
    timeline_repo
        .upsert_item(text_item(conversation_id, &message_id, 1, "drop"))
        .await
        .expect("insert stale item");

    timeline_repo
        .delete_message_items_except_block_indices(&message_id, vec![0])
        .await
        .expect("delete stale message items");

    let remaining = timeline_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("load remaining timeline");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].block_index, 0);
    assert_eq!(remaining[0].text.as_deref(), Some("keep"));
}
