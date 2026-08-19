// Tests for SqliteChatConversationRepository (sqlite_chat_conversation_repo.rs)
// Included via #[cfg(test)] mod in mod.rs

use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AttributionBackfillStatus, AutomationId, ChatContextType,
    ChatConversation, ChatConversationId, CoordinationMode,
};
use crate::domain::repositories::ChatConversationRepository;
use crate::infrastructure::sqlite::SqliteChatConversationRepository;
use crate::testing::SqliteTestDb;
use chrono::Utc;
use std::sync::Arc;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_chat_conversation_repo_tests")
}

/// Build a minimal conversation with a freshly-generated UUID for the ID.
fn make_conversation(context_type: ChatContextType, context_id: &str) -> ChatConversation {
    let now = Utc::now();
    ChatConversation {
        id: ChatConversationId::new(),
        context_type,
        context_id: context_id.to_string(),
        claude_session_id: None,
        provider_session_id: None,
        provider_harness: None,
        upstream_provider: None,
        provider_profile: None,
        agent_mode: None,
        bound_agent_name: None,
        persona_id: None,
        builder_draft_id: None,
        builder_result_persona_id: None,
        coordination_mode: CoordinationMode::Solo,
        automation_id: None,
        automation_run_id: None,
        title: None,
        message_count: 0,
        last_message_at: None,
        created_at: now,
        updated_at: now,
        archived_at: None,
        parent_conversation_id: None,
        attribution_backfill_status: None,
        attribution_backfill_source: None,
        attribution_backfill_source_path: None,
        attribution_backfill_last_attempted_at: None,
        attribution_backfill_completed_at: None,
        attribution_backfill_error_summary: None,
    }
}

#[tokio::test]
async fn create_accepts_valid_standalone_self_key() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let conversation = ChatConversation::new_standalone();

    let created = repo
        .create(conversation.clone())
        .await
        .expect("valid standalone self-key should persist");

    assert_eq!(created.id, conversation.id);
    assert!(created.is_valid_standalone_self_key());
}

#[tokio::test]
async fn create_rejects_invalid_standalone_self_key() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let mut conversation = ChatConversation::new_standalone();
    conversation.context_id = "not-the-conversation-id".to_string();

    let error = repo
        .create(conversation.clone())
        .await
        .expect_err("mismatched standalone self-key must be rejected");

    assert!(matches!(error, crate::error::AppError::Validation(_)));
    assert!(repo.get_by_id(&conversation.id).await.unwrap().is_none());
}

#[tokio::test]
async fn update_persona_binding_sets_and_clears() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let conversation = make_conversation(ChatContextType::Project, "project-persona-binding");
    repo.create(conversation.clone()).await.unwrap();

    repo.update_persona_binding(&conversation.id, Some("persona-1"))
        .await
        .unwrap();
    assert_eq!(
        repo.get_by_id(&conversation.id)
            .await
            .unwrap()
            .unwrap()
            .persona_id
            .as_deref(),
        Some("persona-1")
    );

    repo.update_persona_binding(&conversation.id, None)
        .await
        .unwrap();
    assert_eq!(
        repo.get_by_id(&conversation.id)
            .await
            .unwrap()
            .unwrap()
            .persona_id,
        None
    );
}

#[tokio::test]
async fn bound_agent_name_round_trips_and_updates() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let parent = make_conversation(ChatContextType::Project, "project-bound-agent-parent");
    let parent_id = parent.id.as_str().to_string();
    repo.create(parent).await.unwrap();
    let mut conversation = make_conversation(ChatContextType::Project, "project-bound-agent");
    conversation.parent_conversation_id = Some(parent_id);
    conversation.bound_agent_name = Some("ralphx-workspace-reviewer".to_string());
    repo.create(conversation.clone()).await.unwrap();

    let loaded = repo.get_by_id(&conversation.id).await.unwrap().unwrap();
    assert_eq!(
        loaded.bound_agent_name.as_deref(),
        Some("ralphx-workspace-reviewer")
    );

    repo.update_bound_agent_name(&conversation.id, Some("ralphx-workspace-repair"))
        .await
        .unwrap();
    let updated = repo.get_by_id(&conversation.id).await.unwrap().unwrap();
    assert_eq!(
        updated.bound_agent_name.as_deref(),
        Some("ralphx-workspace-repair")
    );
}

#[tokio::test]
async fn persona_id_round_trips_through_all_select_paths() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO personas (
                 id, slug, name, content, status, content_hash, created_at, updated_at
             ) VALUES (
                 'draft-select-paths', 'draft-select-paths', 'Draft', 'content', 'draft',
                 'hash', '2026-07-15T10:00:00Z', '2026-07-15T10:00:00Z'
             )",
            [],
        )
        .expect("draft fixture");
    });
    let mut conversation = make_conversation(ChatContextType::Project, "project-persona-selects");
    conversation.persona_id = Some("persona-select-paths".to_string());
    conversation.builder_draft_id = Some("draft-select-paths".to_string());
    conversation.automation_id = Some(AutomationId::from_string("automation-persona-selects"));
    conversation.claude_session_id = Some("claude-persona-selects".to_string());
    repo.create(conversation.clone()).await.unwrap();

    let checks = vec![
        repo.get_by_id(&conversation.id).await.unwrap().unwrap(),
        repo.get_by_context(ChatContextType::Project, "project-persona-selects")
            .await
            .unwrap()
            .pop()
            .unwrap(),
        repo.get_by_context_filtered(ChatContextType::Project, "project-persona-selects", true)
            .await
            .unwrap()
            .pop()
            .unwrap(),
        repo.get_by_context_page_filtered(
            ChatContextType::Project,
            "project-persona-selects",
            true,
            false,
            0,
            10,
            None,
        )
        .await
        .unwrap()
        .conversations
        .pop()
        .unwrap(),
        repo.get_active_for_context(ChatContextType::Project, "project-persona-selects")
            .await
            .unwrap()
            .unwrap(),
        repo.list_by_automation_id(conversation.automation_id.as_ref().unwrap())
            .await
            .unwrap()
            .pop()
            .unwrap(),
        repo.list_recent_resumable_by_context_type(ChatContextType::Project, 10)
            .await
            .unwrap()
            .pop()
            .unwrap(),
        repo.list_needing_attribution_backfill(10)
            .await
            .unwrap()
            .pop()
            .unwrap(),
    ];

    for selected in checks {
        assert_eq!(selected.persona_id.as_deref(), Some("persona-select-paths"));
        assert_eq!(
            selected.builder_draft_id.as_deref(),
            Some("draft-select-paths")
        );
    }
}

#[tokio::test]
async fn update_builder_draft_binding_sets_and_clears() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO personas (
                 id, slug, name, content, status, content_hash, created_at, updated_at
             ) VALUES (
                 'draft-1', 'draft-1', 'Draft', 'content', 'draft', 'hash',
                 '2026-07-15T10:00:00Z', '2026-07-15T10:00:00Z'
             )",
            [],
        )
        .expect("draft fixture");
    });
    let conversation = make_conversation(ChatContextType::Project, "project-draft-binding");
    repo.create(conversation.clone()).await.unwrap();

    repo.update_builder_draft_binding(&conversation.id, Some("draft-1"))
        .await
        .unwrap();
    assert_eq!(
        repo.get_by_id(&conversation.id)
            .await
            .unwrap()
            .unwrap()
            .builder_draft_id
            .as_deref(),
        Some("draft-1")
    );

    repo.update_builder_draft_binding(&conversation.id, None)
        .await
        .unwrap();
    assert_eq!(
        repo.get_by_id(&conversation.id)
            .await
            .unwrap()
            .unwrap()
            .builder_draft_id,
        None
    );
}

#[tokio::test]
async fn update_agent_mode_preserves_persona_id() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let mut conversation = make_conversation(ChatContextType::Project, "project-persona-mode");
    conversation.persona_id = Some("persona-mode".to_string());
    repo.create(conversation.clone()).await.unwrap();

    repo.update_agent_mode(&conversation.id, Some(AgentConversationWorkspaceMode::Chat))
        .await
        .unwrap();

    assert_eq!(
        repo.get_by_id(&conversation.id)
            .await
            .unwrap()
            .unwrap()
            .persona_id
            .as_deref(),
        Some("persona-mode")
    );
}

#[tokio::test]
async fn poisoned_persona_id_column_read_fails_closed() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let conversation = make_conversation(ChatContextType::Project, "project-persona-poison");
    repo.create(conversation.clone()).await.unwrap();
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE chat_conversations SET persona_id = X'FF' WHERE id = ?1",
            [conversation.id.as_str()],
        )
        .unwrap();
    });

    let error = repo
        .get_by_id(&conversation.id)
        .await
        .expect_err("BLOB persona IDs must not degrade to None");

    assert!(matches!(error, crate::error::AppError::Database(_)));
}

// --- create ---

#[tokio::test]
async fn test_create_returns_conversation() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Ideation, "ctx-1");
    let conv_id = conv.id.clone();
    let result = repo.create(conv).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id.as_str(), conv_id.as_str());
    assert!(matches!(created.context_type, ChatContextType::Ideation));
}

#[tokio::test]
async fn test_create_preserves_optional_fields() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    // Create a parent conversation first (parent_conversation_id has FK constraint)
    let parent = make_conversation(ChatContextType::Ideation, "ctx-parent");
    let parent_id_str = parent.id.as_str().to_string();
    repo.create(parent).await.unwrap();

    let now = Utc::now();
    let conv = ChatConversation {
        id: ChatConversationId::new(),
        context_type: ChatContextType::Task,
        context_id: "task-42".to_string(),
        claude_session_id: Some("session-xyz".to_string()),
        provider_session_id: Some("session-xyz".to_string()),
        provider_harness: Some(AgentHarnessKind::Claude),
        upstream_provider: Some("anthropic".to_string()),
        provider_profile: Some("default".to_string()),
        agent_mode: Some(AgentConversationWorkspaceMode::Chat),
        bound_agent_name: None,
        persona_id: None,
        builder_draft_id: None,
        builder_result_persona_id: None,
        coordination_mode: CoordinationMode::RxNativeTeam,
        automation_id: None,
        automation_run_id: None,
        title: Some("My Conversation".to_string()),
        message_count: 5,
        last_message_at: Some(now),
        created_at: now,
        updated_at: now,
        archived_at: None,
        parent_conversation_id: Some(parent_id_str.clone()),
        attribution_backfill_status: None,
        attribution_backfill_source: None,
        attribution_backfill_source_path: None,
        attribution_backfill_last_attempted_at: None,
        attribution_backfill_completed_at: None,
        attribution_backfill_error_summary: None,
    };

    repo.create(conv.clone()).await.unwrap();
    let loaded = repo.get_by_id(&conv.id).await.unwrap().unwrap();

    assert_eq!(loaded.claude_session_id, Some("session-xyz".to_string()));
    assert_eq!(loaded.provider_session_id, Some("session-xyz".to_string()));
    assert_eq!(loaded.provider_harness, Some(AgentHarnessKind::Claude));
    assert_eq!(loaded.upstream_provider.as_deref(), Some("anthropic"));
    assert_eq!(loaded.provider_profile.as_deref(), Some("default"));
    assert_eq!(
        loaded.agent_mode,
        Some(AgentConversationWorkspaceMode::Chat)
    );
    assert_eq!(loaded.coordination_mode, CoordinationMode::RxNativeTeam);
    assert_eq!(loaded.title, Some("My Conversation".to_string()));
    assert_eq!(loaded.message_count, 5);
    assert!(loaded.last_message_at.is_some());
    assert_eq!(loaded.parent_conversation_id, Some(parent_id_str));
    assert!(matches!(loaded.context_type, ChatContextType::Task));
}

#[tokio::test]
async fn test_update_coordination_mode_persists_value() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Project, "project-1");
    let conversation_id = conv.id;
    repo.create(conv).await.unwrap();

    repo.update_coordination_mode(&conversation_id, CoordinationMode::RxNativeTeam)
        .await
        .unwrap();

    let loaded = repo.get_by_id(&conversation_id).await.unwrap().unwrap();
    assert_eq!(loaded.coordination_mode, CoordinationMode::RxNativeTeam);
}

// --- get_by_id ---

#[tokio::test]
async fn test_get_by_id_found() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Project, "proj-1");
    let conv_id = conv.id.clone();
    repo.create(conv).await.unwrap();

    let result = repo.get_by_id(&conv_id).await;
    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id.as_str(), conv_id.as_str());
}

#[tokio::test]
async fn test_get_by_id_not_found() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let missing = ChatConversationId::new();
    let result = repo.get_by_id(&missing).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

// --- get_by_context ---

#[tokio::test]
async fn test_get_by_context_returns_matching() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    repo.create(make_conversation(ChatContextType::Task, "task-1"))
        .await
        .unwrap();
    repo.create(make_conversation(ChatContextType::Task, "task-1"))
        .await
        .unwrap();
    repo.create(make_conversation(ChatContextType::Task, "task-2"))
        .await
        .unwrap();
    repo.create(make_conversation(ChatContextType::Ideation, "task-1"))
        .await
        .unwrap();

    let result = repo
        .get_by_context(ChatContextType::Task, "task-1")
        .await
        .unwrap();

    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .all(|c| matches!(c.context_type, ChatContextType::Task)));
    assert!(result.iter().all(|c| c.context_id == "task-1"));
}

#[tokio::test]
async fn test_get_by_context_empty() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let result = repo
        .get_by_context(ChatContextType::Ideation, "no-such-ctx")
        .await
        .unwrap();

    assert!(result.is_empty());
}

#[tokio::test]
async fn test_get_by_context_page_filtered_paginates_and_searches() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let now = Utc::now();

    let mut oldest = make_conversation(ChatContextType::Project, "project-1");
    oldest.title = Some("Oldest agent".to_string());
    oldest.created_at = now - chrono::Duration::minutes(3);
    oldest.updated_at = oldest.created_at;

    let mut middle = make_conversation(ChatContextType::Project, "project-1");
    middle.title = Some("Fix sidebar search".to_string());
    middle.created_at = now - chrono::Duration::minutes(2);
    middle.updated_at = middle.created_at;

    let mut newest = make_conversation(ChatContextType::Project, "project-1");
    newest.title = Some("Newest agent".to_string());
    newest.created_at = now - chrono::Duration::minutes(1);
    newest.updated_at = newest.created_at;

    let mut archived = make_conversation(ChatContextType::Project, "project-1");
    archived.title = Some("Archived sidebar search".to_string());
    archived.created_at = now;
    archived.updated_at = archived.created_at;
    archived.archived_at = Some(now);

    repo.create(oldest.clone()).await.unwrap();
    repo.create(middle.clone()).await.unwrap();
    repo.create(newest.clone()).await.unwrap();
    repo.create(archived.clone()).await.unwrap();

    let page = repo
        .get_by_context_page_filtered(
            ChatContextType::Project,
            "project-1",
            false,
            false,
            0,
            2,
            None,
        )
        .await
        .unwrap();

    assert_eq!(page.total_count, 3);
    assert_eq!(page.limit, 2);
    assert_eq!(page.offset, 0);
    assert!(page.has_more());
    assert_eq!(
        page.conversations
            .iter()
            .map(|conversation| conversation.id.as_str().to_string())
            .collect::<Vec<_>>(),
        vec![
            newest.id.as_str().to_string(),
            middle.id.as_str().to_string(),
        ]
    );

    let second_page = repo
        .get_by_context_page_filtered(
            ChatContextType::Project,
            "project-1",
            false,
            false,
            2,
            2,
            None,
        )
        .await
        .unwrap();

    assert_eq!(second_page.total_count, 3);
    assert!(!second_page.has_more());
    assert_eq!(
        second_page
            .conversations
            .iter()
            .map(|conversation| conversation.id.as_str().to_string())
            .collect::<Vec<_>>(),
        vec![oldest.id.as_str().to_string()]
    );

    let search_page = repo
        .get_by_context_page_filtered(
            ChatContextType::Project,
            "project-1",
            false,
            false,
            0,
            10,
            Some("sidebar search"),
        )
        .await
        .unwrap();

    assert_eq!(search_page.total_count, 1);
    assert_eq!(search_page.conversations[0].id.as_str(), middle.id.as_str());

    let archived_search_page = repo
        .get_by_context_page_filtered(
            ChatContextType::Project,
            "project-1",
            true,
            false,
            0,
            10,
            Some("sidebar search"),
        )
        .await
        .unwrap();

    assert_eq!(archived_search_page.total_count, 2);
    assert_eq!(
        archived_search_page
            .conversations
            .iter()
            .map(|conversation| conversation.id.as_str().to_string())
            .collect::<Vec<_>>(),
        vec![
            archived.id.as_str().to_string(),
            middle.id.as_str().to_string(),
        ]
    );

    let archived_only_page = repo
        .get_by_context_page_filtered(
            ChatContextType::Project,
            "project-1",
            true,
            true,
            0,
            10,
            Some("sidebar search"),
        )
        .await
        .unwrap();

    assert_eq!(archived_only_page.total_count, 1);
    assert_eq!(
        archived_only_page
            .conversations
            .iter()
            .map(|conversation| conversation.id.as_str().to_string())
            .collect::<Vec<_>>(),
        vec![archived.id.as_str().to_string()]
    );
}

// --- get_active_for_context ---

#[tokio::test]
async fn test_get_active_for_context_returns_most_recent() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let old_conv = make_conversation(ChatContextType::Ideation, "ctx-1");
    let old_id = old_conv.id.clone();
    repo.create(old_conv).await.unwrap();

    // Small delay to ensure a different created_at
    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;

    let new_conv = make_conversation(ChatContextType::Ideation, "ctx-1");
    let new_id = new_conv.id.clone();
    repo.create(new_conv).await.unwrap();

    let result = repo
        .get_active_for_context(ChatContextType::Ideation, "ctx-1")
        .await
        .unwrap();

    assert!(result.is_some());
    let found = result.unwrap();
    // Most recent should be returned (ORDER BY created_at DESC LIMIT 1)
    assert_eq!(found.id.as_str(), new_id.as_str());
    assert_ne!(found.id.as_str(), old_id.as_str());
}

#[tokio::test]
async fn test_get_active_for_context_not_found() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let result = repo
        .get_active_for_context(ChatContextType::Merge, "no-such")
        .await
        .unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn test_list_recent_resumable_by_context_type_filters_and_orders() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let now = Utc::now();

    let mut oldest = make_conversation(ChatContextType::Project, "project-oldest");
    oldest.provider_session_id = Some("codex-oldest".to_string());
    oldest.provider_harness = Some(AgentHarnessKind::Codex);
    oldest.last_message_at = Some(now - chrono::Duration::minutes(5));
    oldest.updated_at = now - chrono::Duration::minutes(1);

    let mut newest = make_conversation(ChatContextType::Project, "project-newest");
    newest.claude_session_id = Some("claude-newest".to_string());
    newest.last_message_at = Some(now - chrono::Duration::minutes(1));
    newest.updated_at = now - chrono::Duration::minutes(5);

    let mut middle = make_conversation(ChatContextType::Project, "project-middle");
    middle.provider_session_id = Some("codex-middle".to_string());
    middle.provider_harness = Some(AgentHarnessKind::Codex);
    middle.last_message_at = Some(now - chrono::Duration::minutes(3));

    let missing_session = make_conversation(ChatContextType::Project, "project-missing-session");

    let mut archived = make_conversation(ChatContextType::Project, "project-archived");
    archived.provider_session_id = Some("codex-archived".to_string());
    archived.provider_harness = Some(AgentHarnessKind::Codex);
    archived.archived_at = Some(now);

    let mut task = make_conversation(ChatContextType::Task, "task-1");
    task.provider_session_id = Some("codex-task".to_string());
    task.provider_harness = Some(AgentHarnessKind::Codex);

    repo.create(oldest.clone()).await.unwrap();
    repo.create(newest.clone()).await.unwrap();
    repo.create(middle.clone()).await.unwrap();
    repo.create(missing_session).await.unwrap();
    repo.create(archived).await.unwrap();
    repo.create(task).await.unwrap();

    let resumable = repo
        .list_recent_resumable_by_context_type(ChatContextType::Project, 2)
        .await
        .unwrap();

    assert_eq!(
        resumable
            .iter()
            .map(|conversation| conversation.id.as_str().to_string())
            .collect::<Vec<_>>(),
        vec![
            newest.id.as_str().to_string(),
            middle.id.as_str().to_string(),
        ]
    );
}

#[tokio::test]
async fn test_get_attribution_backfill_summary_counts_legacy_rows() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let mut pending = make_conversation(ChatContextType::Ideation, "ctx-pending");
    pending.claude_session_id = Some("claude-pending".to_string());

    let mut completed = make_conversation(ChatContextType::Ideation, "ctx-completed");
    completed.claude_session_id = Some("claude-completed".to_string());
    completed.attribution_backfill_status = Some(AttributionBackfillStatus::Completed);

    let mut parse_failed = make_conversation(ChatContextType::Ideation, "ctx-parse-failed");
    parse_failed.claude_session_id = Some("claude-parse-failed".to_string());
    parse_failed.attribution_backfill_status = Some(AttributionBackfillStatus::ParseFailed);

    repo.create(pending).await.unwrap();
    repo.create(completed).await.unwrap();
    repo.create(parse_failed).await.unwrap();
    repo.create(make_conversation(
        ChatContextType::Project,
        "ctx-non-legacy",
    ))
    .await
    .unwrap();

    let summary = repo.get_attribution_backfill_summary().await.unwrap();

    assert_eq!(summary.eligible_conversation_count, 3);
    assert_eq!(summary.pending_count, 1);
    assert_eq!(summary.completed_count, 1);
    assert_eq!(summary.parse_failed_count, 1);
    assert_eq!(summary.remaining_count(), 1);
    assert_eq!(summary.attention_count(), 1);
}

// --- update_claude_session_id ---

#[tokio::test]
async fn test_update_claude_session_id() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Ideation, "ctx-1");
    let conv_id = conv.id.clone();
    repo.create(conv).await.unwrap();

    repo.update_claude_session_id(&conv_id, "new-session-id")
        .await
        .unwrap();

    let loaded = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(loaded.claude_session_id, Some("new-session-id".to_string()));
    assert_eq!(
        loaded.provider_session_id,
        Some("new-session-id".to_string())
    );
    assert_eq!(loaded.provider_harness, Some(AgentHarnessKind::Claude));
}

#[tokio::test]
async fn test_update_provider_session_ref_for_codex() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Ideation, "ctx-1");
    let conv_id = conv.id;
    repo.create(conv).await.unwrap();

    repo.update_provider_session_ref(
        &conv_id,
        &ProviderSessionRef {
            harness: AgentHarnessKind::Codex,
            provider_session_id: "codex-session-id".to_string(),
        },
    )
    .await
    .unwrap();

    let loaded = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(loaded.provider_harness, Some(AgentHarnessKind::Codex));
    assert_eq!(
        loaded.provider_session_id,
        Some("codex-session-id".to_string())
    );
    assert_eq!(loaded.claude_session_id, None);
}

#[tokio::test]
async fn refresh_provider_session_ref_updates_existing_ref() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Project, "ctx-refresh-existing");
    let conv_id = conv.id.clone();
    repo.create(conv).await.unwrap();
    repo.update_provider_session_ref(
        &conv_id,
        &ProviderSessionRef {
            harness: AgentHarnessKind::Claude,
            provider_session_id: "session-1".to_string(),
        },
    )
    .await
    .unwrap();

    let refreshed = repo
        .refresh_provider_session_ref(
            &conv_id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: "session-2".to_string(),
            },
        )
        .await
        .unwrap();

    assert!(refreshed, "refresh must apply when a ref is present");
    let loaded = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(
        loaded.provider_session_ref().map(|r| r.provider_session_id),
        Some("session-2".to_string())
    );
}

#[tokio::test]
async fn refresh_provider_session_ref_never_resurrects_cleared_ref() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Project, "ctx-refresh-cleared");
    let conv_id = conv.id.clone();
    repo.create(conv).await.unwrap();
    repo.update_provider_session_ref(
        &conv_id,
        &ProviderSessionRef {
            harness: AgentHarnessKind::Claude,
            provider_session_id: "plan-session".to_string(),
        },
    )
    .await
    .unwrap();
    repo.clear_provider_session_ref(&conv_id).await.unwrap();

    let refreshed = repo
        .refresh_provider_session_ref(
            &conv_id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: "plan-session".to_string(),
            },
        )
        .await
        .unwrap();

    assert!(!refreshed, "cleared ref must not be resurrected");
    let loaded = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert!(loaded.provider_session_ref().is_none());
    assert_eq!(loaded.claude_session_id, None);
    assert_eq!(loaded.provider_session_id, None);
    assert_eq!(loaded.provider_harness, None);
}

#[tokio::test]
async fn refresh_provider_session_ref_honors_legacy_claude_only_rows() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Project, "ctx-refresh-legacy");
    let conv_id = conv.id.clone();
    repo.create(conv).await.unwrap();
    // Legacy shape: only `claude_session_id` is set, matching pre-provider-harness rows.
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE chat_conversations
                SET claude_session_id = ?1,
                    provider_session_id = NULL,
                    provider_harness = NULL
              WHERE id = ?2",
            rusqlite::params!["legacy-session", conv_id.as_str()],
        )
        .unwrap();
    });

    let refreshed = repo
        .refresh_provider_session_ref(
            &conv_id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: "legacy-session-next".to_string(),
            },
        )
        .await
        .unwrap();

    assert!(refreshed, "legacy claude-only rows count as present");
    let loaded = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(
        loaded.provider_session_ref().map(|r| r.provider_session_id),
        Some("legacy-session-next".to_string())
    );
}

#[tokio::test]
async fn test_update_role_default_bindings_updates_mode_persona_and_session_tuple() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let mut conversation = make_conversation(ChatContextType::Project, "project-role-bindings");
    conversation.persona_id = Some("old-persona".to_string());
    conversation.claude_session_id = Some("old-claude-session".to_string());
    conversation.provider_session_id = Some("old-provider-session".to_string());
    conversation.provider_harness = Some(AgentHarnessKind::Claude);
    let conversation_id = conversation.id.clone();
    repo.create(conversation).await.unwrap();

    repo.update_role_default_bindings(
        &conversation_id,
        CoordinationMode::RxNativeWorkflow,
        Some("new-persona"),
        true,
    )
    .await
    .unwrap();

    let loaded = repo.get_by_id(&conversation_id).await.unwrap().unwrap();
    assert_eq!(loaded.coordination_mode, CoordinationMode::RxNativeWorkflow);
    assert_eq!(loaded.persona_id.as_deref(), Some("new-persona"));
    assert!(loaded.claude_session_id.is_none());
    assert!(loaded.provider_session_id.is_none());
    assert!(loaded.provider_harness.is_none());
}

#[tokio::test]
async fn test_update_role_default_bindings_preserves_session_tuple_when_requested() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let mut conversation = make_conversation(ChatContextType::Project, "project-role-bindings");
    conversation.claude_session_id = Some("keep-claude-session".to_string());
    conversation.provider_session_id = Some("keep-provider-session".to_string());
    conversation.provider_harness = Some(AgentHarnessKind::Claude);
    let conversation_id = conversation.id.clone();
    repo.create(conversation).await.unwrap();

    repo.update_role_default_bindings(&conversation_id, CoordinationMode::Solo, None, false)
        .await
        .unwrap();

    let loaded = repo.get_by_id(&conversation_id).await.unwrap().unwrap();
    assert_eq!(loaded.coordination_mode, CoordinationMode::Solo);
    assert_eq!(loaded.persona_id, None);
    assert_eq!(
        loaded.claude_session_id.as_deref(),
        Some("keep-claude-session")
    );
    assert_eq!(
        loaded.provider_session_id.as_deref(),
        Some("keep-provider-session")
    );
    assert_eq!(loaded.provider_harness, Some(AgentHarnessKind::Claude));
}

#[tokio::test]
async fn test_update_agent_mode_and_role_bindings_persists_one_tuple() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let conversation = make_conversation(ChatContextType::Project, "project-atomic-edit");
    let conversation_id = conversation.id;
    repo.create(conversation).await.unwrap();

    repo.update_agent_mode_and_role_default_bindings(
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        CoordinationMode::RxNativeWorkflow,
        Some("persona-edit"),
        false,
    )
    .await
    .unwrap();

    let loaded = repo.get_by_id(&conversation_id).await.unwrap().unwrap();
    assert_eq!(
        loaded.agent_mode,
        Some(AgentConversationWorkspaceMode::Edit)
    );
    assert_eq!(loaded.coordination_mode, CoordinationMode::RxNativeWorkflow);
    assert_eq!(loaded.persona_id.as_deref(), Some("persona-edit"));
}

// --- clear_claude_session_id ---

#[tokio::test]
async fn test_clear_claude_session_id() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let now = Utc::now();
    let conv = ChatConversation {
        id: ChatConversationId::new(),
        context_type: ChatContextType::Ideation,
        context_id: "ctx-1".to_string(),
        claude_session_id: Some("existing-session".to_string()),
        provider_session_id: Some("existing-session".to_string()),
        provider_harness: Some(AgentHarnessKind::Claude),
        upstream_provider: None,
        provider_profile: None,
        agent_mode: None,
        bound_agent_name: None,
        persona_id: None,
        builder_draft_id: None,
        builder_result_persona_id: None,
        coordination_mode: CoordinationMode::Solo,
        automation_id: None,
        automation_run_id: None,
        title: None,
        message_count: 0,
        last_message_at: None,
        created_at: now,
        updated_at: now,
        archived_at: None,
        parent_conversation_id: None,
        attribution_backfill_status: None,
        attribution_backfill_source: None,
        attribution_backfill_source_path: None,
        attribution_backfill_last_attempted_at: None,
        attribution_backfill_completed_at: None,
        attribution_backfill_error_summary: None,
    };
    let conv_id = conv.id.clone();
    repo.create(conv).await.unwrap();

    repo.clear_claude_session_id(&conv_id).await.unwrap();

    let loaded = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert!(loaded.claude_session_id.is_none());
    assert!(loaded.provider_session_id.is_none());
    assert!(loaded.provider_harness.is_none());
}

#[tokio::test]
async fn test_update_provider_origin() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Ideation, "ctx-1");
    let conv_id = conv.id.clone();
    repo.create(conv).await.unwrap();

    repo.update_provider_origin(&conv_id, Some("z_ai"), Some("z_ai"))
        .await
        .unwrap();

    let loaded = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(loaded.upstream_provider.as_deref(), Some("z_ai"));
    assert_eq!(loaded.provider_profile.as_deref(), Some("z_ai"));
}

// --- update_title ---

#[tokio::test]
async fn test_update_title() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Ideation, "ctx-1");
    let conv_id = conv.id.clone();
    repo.create(conv).await.unwrap();

    repo.update_title(&conv_id, "My New Title").await.unwrap();

    let loaded = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(loaded.title, Some("My New Title".to_string()));
}

// --- archive / restore ---

#[tokio::test]
async fn test_archive_filters_from_default_context_queries() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Project, "project-1");
    let conv_id = conv.id.clone();
    repo.create(conv).await.unwrap();

    repo.archive(&conv_id).await.unwrap();

    let loaded = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert!(loaded.archived_at.is_some());
    assert!(repo
        .get_by_context(ChatContextType::Project, "project-1")
        .await
        .unwrap()
        .is_empty());
    assert!(repo
        .get_by_context_filtered(ChatContextType::Project, "project-1", true)
        .await
        .unwrap()
        .iter()
        .any(|conversation| conversation.id == conv_id));
}

#[tokio::test]
async fn test_restore_returns_conversation_to_default_context_queries() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Project, "project-1");
    let conv_id = conv.id.clone();
    repo.create(conv).await.unwrap();

    repo.archive(&conv_id).await.unwrap();
    repo.restore(&conv_id).await.unwrap();

    let loaded = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert!(loaded.archived_at.is_none());
    assert_eq!(
        repo.get_by_context(ChatContextType::Project, "project-1")
            .await
            .unwrap()
            .len(),
        1
    );
}

// --- update_message_stats ---

#[tokio::test]
async fn test_update_message_stats() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Task, "task-1");
    let conv_id = conv.id.clone();
    repo.create(conv).await.unwrap();

    let last_msg_at = Utc::now();
    repo.update_message_stats(&conv_id, 42, last_msg_at)
        .await
        .unwrap();

    let loaded = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(loaded.message_count, 42);
    assert!(loaded.last_message_at.is_some());
}

// --- delete ---

#[tokio::test]
async fn test_delete_removes_conversation() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = make_conversation(ChatContextType::Ideation, "ctx-1");
    let conv_id = conv.id.clone();
    repo.create(conv).await.unwrap();

    repo.delete(&conv_id).await.unwrap();

    let found = repo.get_by_id(&conv_id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_delete_nonexistent_is_ok() {
    // delete does not return an error for missing conversations (no affected-rows check)
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let missing = ChatConversationId::new();
    let result = repo.delete(&missing).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_list_needing_attribution_backfill_only_returns_pending_rows() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let mut pending = make_conversation(ChatContextType::Ideation, "ctx-pending");
    pending.claude_session_id = Some("claude-pending".to_string());
    let pending_id = pending.id;

    let mut running = make_conversation(ChatContextType::Ideation, "ctx-running");
    running.claude_session_id = Some("claude-running".to_string());
    running.attribution_backfill_status = Some(AttributionBackfillStatus::Running);

    let mut partial = make_conversation(ChatContextType::Ideation, "ctx-partial");
    partial.claude_session_id = Some("claude-partial".to_string());
    partial.attribution_backfill_status = Some(AttributionBackfillStatus::Partial);

    repo.create(pending).await.unwrap();
    repo.create(running).await.unwrap();
    repo.create(partial).await.unwrap();

    let needing = repo.list_needing_attribution_backfill(10).await.unwrap();
    assert_eq!(needing.len(), 1);
    assert_eq!(needing[0].id, pending_id);
}

#[tokio::test]
async fn test_reset_running_attribution_backfill_to_pending() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let mut running = make_conversation(ChatContextType::Ideation, "ctx-running");
    running.claude_session_id = Some("claude-running".to_string());
    running.attribution_backfill_status = Some(AttributionBackfillStatus::Running);
    let running_id = running.id;

    let mut completed = make_conversation(ChatContextType::Ideation, "ctx-completed");
    completed.claude_session_id = Some("claude-completed".to_string());
    completed.attribution_backfill_status = Some(AttributionBackfillStatus::Completed);

    repo.create(running).await.unwrap();
    repo.create(completed).await.unwrap();

    let reset_count = repo
        .reset_running_attribution_backfill_to_pending()
        .await
        .unwrap();
    assert_eq!(reset_count, 1);

    let updated = repo.get_by_id(&running_id).await.unwrap().unwrap();
    assert_eq!(
        updated.attribution_backfill_status,
        Some(AttributionBackfillStatus::Pending)
    );
}

// --- delete_by_context ---

#[tokio::test]
async fn test_delete_by_context_removes_all_matching() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    repo.create(make_conversation(ChatContextType::Review, "review-1"))
        .await
        .unwrap();
    repo.create(make_conversation(ChatContextType::Review, "review-1"))
        .await
        .unwrap();
    repo.create(make_conversation(ChatContextType::Review, "review-2"))
        .await
        .unwrap();

    repo.delete_by_context(ChatContextType::Review, "review-1")
        .await
        .unwrap();

    let remaining = repo
        .get_by_context(ChatContextType::Review, "review-1")
        .await
        .unwrap();
    assert!(remaining.is_empty());

    // Other context_id unaffected
    let other = repo
        .get_by_context(ChatContextType::Review, "review-2")
        .await
        .unwrap();
    assert_eq!(other.len(), 1);
}

// --- parse_datetime edge cases (raw SQL) ---

#[tokio::test]
async fn test_parse_datetime_bad_format_falls_back_to_now() {
    // parse_datetime silently falls back to Utc::now() on bad input — verify no panic
    let db = setup_test_db();
    let id = ChatConversationId::new();
    let id_str = id.as_str().to_string();

    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations \
             (id, context_type, context_id, message_count, created_at, updated_at) \
             VALUES (?1, 'ideation', 'ctx-bad', 0, 'not-a-datetime', 'also-not-datetime')",
            rusqlite::params![id_str],
        )
        .unwrap();
    });

    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let result = repo.get_by_id(&id).await;
    assert!(result.is_ok(), "Should not panic on bad timestamp");
    // Should return Some (fallback to now rather than erroring)
    assert!(result.unwrap().is_some());
}

#[tokio::test]
async fn test_unknown_context_type_defaults_to_ideation() {
    // context_type_str.parse().unwrap_or(Ideation) — unknown type silently becomes Ideation
    let db = setup_test_db();
    let id = ChatConversationId::new();
    let id_str = id.as_str().to_string();

    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations \
             (id, context_type, context_id, message_count, created_at, updated_at) \
             VALUES (?1, 'totally_unknown_type', 'ctx-1', 0, ?2, ?3)",
            rusqlite::params![id_str, Utc::now().to_rfc3339(), Utc::now().to_rfc3339()],
        )
        .unwrap();
    });

    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let conv = repo.get_by_id(&id).await.unwrap().unwrap();
    assert!(
        matches!(conv.context_type, ChatContextType::Ideation),
        "Unknown context_type should default to Ideation"
    );
}

// --- context type round-trips ---

#[tokio::test]
async fn test_all_context_types_round_trip() {
    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let types_and_ids: Vec<(ChatConversation, ChatContextType)> = vec![
        (
            make_conversation(ChatContextType::Ideation, "ctx"),
            ChatContextType::Ideation,
        ),
        (
            make_conversation(ChatContextType::Task, "ctx"),
            ChatContextType::Task,
        ),
        (
            make_conversation(ChatContextType::Project, "ctx"),
            ChatContextType::Project,
        ),
        (
            make_conversation(ChatContextType::Review, "ctx"),
            ChatContextType::Review,
        ),
        (
            make_conversation(ChatContextType::Merge, "ctx"),
            ChatContextType::Merge,
        ),
    ];

    for (conv, _) in &types_and_ids {
        repo.create(conv.clone()).await.unwrap();
    }

    for (conv, expected_type) in &types_and_ids {
        let loaded = repo.get_by_id(&conv.id).await.unwrap().unwrap();
        assert_eq!(
            std::mem::discriminant(&loaded.context_type),
            std::mem::discriminant(expected_type),
            "Context type mismatch for conv {:?}",
            conv.id.as_str()
        );
    }
}

// --- from_shared_connection ---

#[tokio::test]
async fn test_from_shared_connection() {
    let db = setup_test_db();
    let shared = db.shared_conn();

    let repo1 = SqliteChatConversationRepository::from_shared(Arc::clone(&shared));
    let repo2 = SqliteChatConversationRepository::from_shared(Arc::clone(&shared));

    let conv = make_conversation(ChatContextType::Ideation, "ctx-shared");
    let conv_id = conv.id.clone();
    repo1.create(conv).await.unwrap();

    let found = repo2.get_by_id(&conv_id).await.unwrap();
    assert!(found.is_some());
}

// --- list_by_automation_id ---

#[tokio::test]
async fn test_list_by_automation_id_returns_setup_and_run_conversations() {
    use crate::domain::entities::{AutomationId, AutomationRunId};

    let db = setup_test_db();
    let repo = SqliteChatConversationRepository::from_shared(db.shared_conn());

    let automation_id = AutomationId::from_string("automation-list-1");

    // Setup conversation: automation_id only.
    let mut setup = make_conversation(ChatContextType::Project, "project-1");
    setup.automation_id = Some(automation_id.clone());
    let setup_id = setup.id.clone();
    repo.create(setup).await.unwrap();

    // Run conversation: both automation_id and automation_run_id.
    let mut run_conv = make_conversation(ChatContextType::Project, "project-1");
    run_conv.automation_id = Some(automation_id.clone());
    run_conv.automation_run_id = Some(AutomationRunId::from_string("run-1"));
    let run_id = run_conv.id.clone();
    repo.create(run_conv).await.unwrap();

    // Archived conversation for the same automation is still returned (filtering
    // happens in the delete flow, not the repo query).
    let mut archived = make_conversation(ChatContextType::Project, "project-1");
    archived.automation_id = Some(automation_id.clone());
    let archived_id = archived.id.clone();
    repo.create(archived).await.unwrap();
    repo.archive(&archived_id).await.unwrap();

    // Unrelated conversation for a different automation must be excluded.
    let mut other = make_conversation(ChatContextType::Project, "project-1");
    other.automation_id = Some(AutomationId::from_string("automation-other"));
    repo.create(other).await.unwrap();

    let listed = repo.list_by_automation_id(&automation_id).await.unwrap();
    let ids: Vec<String> = listed.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&setup_id.as_str()));
    assert!(ids.contains(&run_id.as_str()));
    assert!(ids.contains(&archived_id.as_str()));
}
