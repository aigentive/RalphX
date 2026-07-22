// Tests for SqliteChatMessageRepository

use super::sqlite_chat_message_repo::SqliteChatMessageRepository;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort, ProviderSessionRef};
use crate::domain::entities::{
    AgentRunUsage, ChatMessage, ChatMessageAttribution, ChatMessageId, IdeationSession,
    IdeationSessionId, ProjectId, TaskId,
};
use crate::domain::repositories::ChatMessageRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite-chat-message-repo")
}

fn create_test_project(db: &SqliteTestDb, id: &ProjectId, name: &str, path: &str) {
    db.with_connection(|conn| {
        conn.execute(
                "INSERT INTO projects (id, name, working_directory, git_mode, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'single_branch', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
                rusqlite::params![id.as_str(), name, path],
            )
            .unwrap();
    });
}

fn create_test_session(db: &SqliteTestDb, project_id: &ProjectId) -> IdeationSessionId {
    let session = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Test Session")
        .build();

    db.with_connection(|conn| {
        conn.execute(
                "INSERT INTO ideation_sessions (id, project_id, title, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'active', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
                rusqlite::params![
                    session.id.as_str(),
                    project_id.as_str(),
                    "Test Session"
                ],
            )
            .unwrap();
    });

    session.id
}

fn create_test_task(db: &SqliteTestDb, project_id: &ProjectId) -> TaskId {
    let task_id = TaskId::new();
    db.with_connection(|conn| {
        conn.execute(
                "INSERT INTO tasks (id, project_id, category, title, description, internal_status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
                rusqlite::params![
                    task_id.as_str(),
                    project_id.as_str(),
                    "feature",
                    "Test Task",
                    "",
                    "backlog",
                ],
            )
            .unwrap();
    });
    task_id
}

// ==================== CREATE TESTS ====================

#[tokio::test]
async fn test_create_inserts_message_and_returns_it() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let message = ChatMessage::user_in_session(session_id.clone(), "Hello, world!");

    let result = repo.create(message.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id, message.id);
    assert_eq!(created.content, "Hello, world!");
    assert_eq!(created.session_id, Some(session_id));
}

#[tokio::test]
async fn test_create_message_with_metadata() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let message = ChatMessage::user_in_session(session_id, "With metadata")
        .with_metadata(r#"{"key": "value"}"#);

    let result = repo.create(message.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.metadata, Some(r#"{"key": "value"}"#.to_string()));
}

#[tokio::test]
async fn test_create_message_with_parent() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    // Create parent message
    let parent = ChatMessage::user_in_session(session_id.clone(), "Parent message");
    repo.create(parent.clone()).await.unwrap();

    // Create child message
    let child = ChatMessage::orchestrator_in_session(session_id, "Reply to parent")
        .with_parent(parent.id.clone());
    let result = repo.create(child.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.parent_message_id, Some(parent.id));
}

#[tokio::test]
async fn test_create_duplicate_id_fails() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let message = ChatMessage::user_in_session(session_id, "Duplicate");

    repo.create(message.clone()).await.unwrap();
    let result = repo.create(message).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_create_project_message() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let message = ChatMessage::user_in_project(project_id.clone(), "Project-level chat");

    let result = repo.create(message.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.project_id, Some(project_id));
    assert!(created.session_id.is_none());
}

#[tokio::test]
async fn test_create_task_message() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let task_id = create_test_task(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let message = ChatMessage::user_about_task(task_id.clone(), "Task-specific chat");

    let result = repo.create(message.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.task_id, Some(task_id));
}

// ==================== GET BY ID TESTS ====================

#[tokio::test]
async fn test_get_by_id_retrieves_message_correctly() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let message = ChatMessage::user_in_session(session_id.clone(), "Find me");

    repo.create(message.clone()).await.unwrap();
    let result = repo.get_by_id(&message.id).await;

    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_some());
    let found_message = found.unwrap();
    assert_eq!(found_message.id, message.id);
    assert_eq!(found_message.content, "Find me");
    assert_eq!(found_message.session_id, Some(session_id));
}

#[tokio::test]
async fn test_get_by_id_returns_none_for_nonexistent() {
    let db = setup_test_db();
    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let id = ChatMessageId::new();

    let result = repo.get_by_id(&id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_get_by_id_preserves_all_fields() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let message = ChatMessage::orchestrator_in_session(session_id.clone(), "Full message")
        .with_metadata(r#"{"context": "test"}"#);

    repo.create(message.clone()).await.unwrap();
    let found = repo.get_by_id(&message.id).await.unwrap().unwrap();

    assert_eq!(found.id, message.id);
    assert_eq!(found.session_id, Some(session_id));
    assert_eq!(found.content, "Full message");
    assert_eq!(found.metadata, Some(r#"{"context": "test"}"#.to_string()));
    assert!(found.is_orchestrator());
}

#[tokio::test]
async fn test_get_by_id_roundtrips_message_attribution_fields() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let message = ChatMessage::orchestrator_in_session(session_id, "Attributed message")
        .with_attribution(ChatMessageAttribution {
            attribution_source: Some("native_runtime".to_string()),
            provider_harness: Some(AgentHarnessKind::Codex),
            provider_session_id: Some("codex-session-123".to_string()),
            upstream_provider: Some("openai".to_string()),
            provider_profile: None,
            logical_model: Some("gpt-5.4".to_string()),
            effective_model_id: Some("gpt-5.4".to_string()),
            logical_effort: Some(LogicalEffort::High),
            effective_effort: Some("high".to_string()),
        });

    repo.create(message.clone()).await.unwrap();
    let found = repo.get_by_id(&message.id).await.unwrap().unwrap();

    assert_eq!(found.attribution_source, Some("native_runtime".to_string()));
    assert_eq!(found.provider_harness, Some(AgentHarnessKind::Codex));
    assert_eq!(
        found.provider_session_id,
        Some("codex-session-123".to_string())
    );
    assert_eq!(found.upstream_provider.as_deref(), Some("openai"));
    assert_eq!(found.provider_profile, None);
    assert_eq!(found.logical_model, Some("gpt-5.4".to_string()));
    assert_eq!(found.effective_model_id, Some("gpt-5.4".to_string()));
    assert_eq!(found.logical_effort, Some(LogicalEffort::High));
    assert_eq!(found.effective_effort, Some("high".to_string()));
}

#[tokio::test]
async fn test_update_provider_session_ref_updates_message_provider_fields() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let message = ChatMessage::orchestrator_in_session(session_id, "Provider ref update");
    repo.create(message.clone()).await.unwrap();

    repo.update_provider_session_ref(
        &message.id,
        &ProviderSessionRef {
            harness: AgentHarnessKind::Codex,
            provider_session_id: "codex-session-456".to_string(),
        },
    )
    .await
    .unwrap();

    let found = repo.get_by_id(&message.id).await.unwrap().unwrap();
    assert_eq!(found.provider_harness, Some(AgentHarnessKind::Codex));
    assert_eq!(
        found.provider_session_id,
        Some("codex-session-456".to_string())
    );
}

#[tokio::test]
async fn test_get_by_id_roundtrips_message_usage_fields() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let mut message = ChatMessage::orchestrator_in_session(session_id, "Usage message");
    message.apply_usage(&AgentRunUsage {
        input_tokens: Some(120),
        output_tokens: Some(35),
        cache_creation_tokens: Some(12),
        cache_read_tokens: Some(48),
        estimated_usd: Some(0.042),
    });

    repo.create(message.clone()).await.unwrap();
    let found = repo.get_by_id(&message.id).await.unwrap().unwrap();

    assert_eq!(found.input_tokens, Some(120));
    assert_eq!(found.output_tokens, Some(35));
    assert_eq!(found.cache_creation_tokens, Some(12));
    assert_eq!(found.cache_read_tokens, Some(48));
    assert_eq!(found.estimated_usd, Some(0.042));
}

#[tokio::test]
async fn test_update_usage_updates_message_usage_fields() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let message = ChatMessage::orchestrator_in_session(session_id, "Usage update");
    repo.create(message.clone()).await.unwrap();

    repo.update_usage(
        &message.id,
        &AgentRunUsage {
            input_tokens: Some(200),
            output_tokens: Some(60),
            cache_creation_tokens: Some(15),
            cache_read_tokens: Some(70),
            estimated_usd: Some(0.19),
        },
    )
    .await
    .unwrap();

    let found = repo.get_by_id(&message.id).await.unwrap().unwrap();
    assert_eq!(found.input_tokens, Some(200));
    assert_eq!(found.output_tokens, Some(60));
    assert_eq!(found.cache_creation_tokens, Some(15));
    assert_eq!(found.cache_read_tokens, Some(70));
    assert_eq!(found.estimated_usd, Some(0.19));
}

#[tokio::test]
async fn test_update_attribution_updates_message_attribution_fields() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let message = ChatMessage::orchestrator_in_session(session_id, "Attribution update");
    repo.create(message.clone()).await.unwrap();

    repo.update_attribution(
        &message.id,
        &ChatMessageAttribution {
            attribution_source: Some(
                "historical_backfill_claude_project_jsonl_anthropic".to_string(),
            ),
            provider_harness: Some(AgentHarnessKind::Claude),
            provider_session_id: Some("claude-session-999".to_string()),
            upstream_provider: Some("anthropic".to_string()),
            provider_profile: Some("default".to_string()),
            logical_model: Some("claude-sonnet-4-6".to_string()),
            effective_model_id: Some("claude-sonnet-4-6".to_string()),
            logical_effort: Some(LogicalEffort::Medium),
            effective_effort: Some("medium".to_string()),
        },
    )
    .await
    .unwrap();

    let found = repo.get_by_id(&message.id).await.unwrap().unwrap();
    assert_eq!(
        found.attribution_source.as_deref(),
        Some("historical_backfill_claude_project_jsonl_anthropic")
    );
    assert_eq!(found.provider_harness, Some(AgentHarnessKind::Claude));
    assert_eq!(
        found.provider_session_id.as_deref(),
        Some("claude-session-999")
    );
    assert_eq!(found.upstream_provider.as_deref(), Some("anthropic"));
    assert_eq!(found.provider_profile.as_deref(), Some("default"));
    assert_eq!(found.logical_model.as_deref(), Some("claude-sonnet-4-6"));
    assert_eq!(
        found.effective_model_id.as_deref(),
        Some("claude-sonnet-4-6")
    );
    assert_eq!(found.logical_effort, Some(LogicalEffort::Medium));
    assert_eq!(found.effective_effort.as_deref(), Some("medium"));
}

// ==================== GET BY SESSION TESTS ====================

#[tokio::test]
async fn test_get_by_session_returns_all_messages() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_in_session(session_id.clone(), "First");
    let msg2 = ChatMessage::orchestrator_in_session(session_id.clone(), "Second");
    let msg3 = ChatMessage::user_in_session(session_id.clone(), "Third");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();
    repo.create(msg3).await.unwrap();

    let result = repo.get_by_session(&session_id).await;

    assert!(result.is_ok());
    let messages = result.unwrap();
    assert_eq!(messages.len(), 3);
}

#[tokio::test]
async fn test_get_by_session_ordered_by_created_at_asc() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    // Create messages with slight delays to ensure different timestamps
    let msg1 = ChatMessage::user_in_session(session_id.clone(), "First");
    repo.create(msg1.clone()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    let msg2 = ChatMessage::orchestrator_in_session(session_id.clone(), "Second");
    repo.create(msg2.clone()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    let msg3 = ChatMessage::user_in_session(session_id.clone(), "Third");
    repo.create(msg3.clone()).await.unwrap();

    let messages = repo.get_by_session(&session_id).await.unwrap();

    // Should be in ascending order (oldest first)
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].content, "First");
    assert_eq!(messages[1].content, "Second");
    assert_eq!(messages[2].content, "Third");
}

#[tokio::test]
async fn test_get_by_session_returns_empty_for_no_messages() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let result = repo.get_by_session(&session_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_session_filters_by_session() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id1 = create_test_session(&db, &project_id);
    let session_id2 = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_in_session(session_id1.clone(), "Session 1 message");
    let msg2 = ChatMessage::user_in_session(session_id2.clone(), "Session 2 message");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();

    let messages = repo.get_by_session(&session_id1).await.unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].session_id, Some(session_id1));
}

// ==================== GET BY PROJECT TESTS ====================

#[tokio::test]
async fn test_get_by_project_returns_project_messages_only() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    // Create a project message (no session)
    let project_msg = ChatMessage::user_in_project(project_id.clone(), "Project chat");
    // Create a session message
    let session_msg = ChatMessage::user_in_session(session_id.clone(), "Session chat");

    repo.create(project_msg).await.unwrap();
    repo.create(session_msg).await.unwrap();

    let result = repo.get_by_project(&project_id).await;

    assert!(result.is_ok());
    let messages = result.unwrap();
    // Should only return the project message, not the session message
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "Project chat");
    assert!(messages[0].session_id.is_none());
}

#[tokio::test]
async fn test_get_by_project_returns_empty_for_no_messages() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let result = repo.get_by_project(&project_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_project_filters_by_project() {
    let db = setup_test_db();
    let project_id1 = ProjectId::new();
    let project_id2 = ProjectId::new();
    create_test_project(&db, &project_id1, "Project 1", "/path1");
    create_test_project(&db, &project_id2, "Project 2", "/path2");

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_in_project(project_id1.clone(), "P1 message");
    let msg2 = ChatMessage::user_in_project(project_id2.clone(), "P2 message");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();

    let messages = repo.get_by_project(&project_id1).await.unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].project_id, Some(project_id1));
}

// ==================== GET BY TASK TESTS ====================

#[tokio::test]
async fn test_get_by_task_returns_task_messages() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let task_id = create_test_task(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_about_task(task_id.clone(), "Task question");
    let msg2 = ChatMessage::user_about_task(task_id.clone(), "Follow-up");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();

    let result = repo.get_by_task(&task_id).await;

    assert!(result.is_ok());
    let messages = result.unwrap();
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_get_by_task_returns_empty_for_no_messages() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let task_id = create_test_task(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let result = repo.get_by_task(&task_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_task_filters_by_task() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let task_id1 = create_test_task(&db, &project_id);
    let task_id2 = create_test_task(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_about_task(task_id1.clone(), "Task 1 msg");
    let msg2 = ChatMessage::user_about_task(task_id2.clone(), "Task 2 msg");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();

    let messages = repo.get_by_task(&task_id1).await.unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].task_id, Some(task_id1));
}

// ==================== DELETE BY SESSION TESTS ====================

#[tokio::test]
async fn test_delete_by_session_removes_all_session_messages() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_in_session(session_id.clone(), "Message 1");
    let msg2 = ChatMessage::orchestrator_in_session(session_id.clone(), "Message 2");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();

    let delete_result = repo.delete_by_session(&session_id).await;
    assert!(delete_result.is_ok());

    let messages = repo.get_by_session(&session_id).await.unwrap();
    assert!(messages.is_empty());
}

#[tokio::test]
async fn test_delete_by_session_does_not_affect_other_sessions() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id1 = create_test_session(&db, &project_id);
    let session_id2 = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_in_session(session_id1.clone(), "Session 1");
    let msg2 = ChatMessage::user_in_session(session_id2.clone(), "Session 2");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();

    repo.delete_by_session(&session_id1).await.unwrap();

    let session1_messages = repo.get_by_session(&session_id1).await.unwrap();
    let session2_messages = repo.get_by_session(&session_id2).await.unwrap();

    assert!(session1_messages.is_empty());
    assert_eq!(session2_messages.len(), 1);
}

#[tokio::test]
async fn test_delete_by_session_nonexistent_succeeds() {
    let db = setup_test_db();
    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let session_id = IdeationSessionId::new();

    let result = repo.delete_by_session(&session_id).await;
    assert!(result.is_ok());
}

// ==================== DELETE BY PROJECT TESTS ====================

#[tokio::test]
async fn test_delete_by_project_removes_all_project_messages() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_in_project(project_id.clone(), "Message 1");
    let msg2 = ChatMessage::user_in_project(project_id.clone(), "Message 2");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();

    let delete_result = repo.delete_by_project(&project_id).await;
    assert!(delete_result.is_ok());

    let messages = repo.get_by_project(&project_id).await.unwrap();
    assert!(messages.is_empty());
}

// ==================== DELETE BY TASK TESTS ====================

#[tokio::test]
async fn test_delete_by_task_removes_all_task_messages() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let task_id = create_test_task(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_about_task(task_id.clone(), "Message 1");
    let msg2 = ChatMessage::user_about_task(task_id.clone(), "Message 2");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();

    let delete_result = repo.delete_by_task(&task_id).await;
    assert!(delete_result.is_ok());

    let messages = repo.get_by_task(&task_id).await.unwrap();
    assert!(messages.is_empty());
}

// ==================== DELETE SINGLE MESSAGE TESTS ====================

#[tokio::test]
async fn test_delete_removes_single_message() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_in_session(session_id.clone(), "Keep");
    let msg2 = ChatMessage::user_in_session(session_id.clone(), "Delete");

    repo.create(msg1.clone()).await.unwrap();
    repo.create(msg2.clone()).await.unwrap();

    let delete_result = repo.delete(&msg2.id).await;
    assert!(delete_result.is_ok());

    let found = repo.get_by_id(&msg2.id).await.unwrap();
    assert!(found.is_none());

    // Other message should still exist
    let kept = repo.get_by_id(&msg1.id).await.unwrap();
    assert!(kept.is_some());
}

#[tokio::test]
async fn test_delete_nonexistent_succeeds() {
    let db = setup_test_db();
    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let id = ChatMessageId::new();

    let result = repo.delete(&id).await;
    assert!(result.is_ok());
}

// ==================== COUNT BY SESSION TESTS ====================

#[tokio::test]
async fn test_count_by_session_returns_zero_for_no_messages() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let result = repo.count_by_session(&session_id).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}

#[tokio::test]
async fn test_count_by_session_counts_correctly() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_in_session(session_id.clone(), "One");
    let msg2 = ChatMessage::orchestrator_in_session(session_id.clone(), "Two");
    let msg3 = ChatMessage::user_in_session(session_id.clone(), "Three");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();
    repo.create(msg3).await.unwrap();

    let result = repo.count_by_session(&session_id).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 3);
}

#[tokio::test]
async fn test_count_by_session_filters_by_session() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id1 = create_test_session(&db, &project_id);
    let session_id2 = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_in_session(session_id1.clone(), "S1-1");
    let msg2 = ChatMessage::user_in_session(session_id1.clone(), "S1-2");
    let msg3 = ChatMessage::user_in_session(session_id2.clone(), "S2-1");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();
    repo.create(msg3).await.unwrap();

    let count1 = repo.count_by_session(&session_id1).await.unwrap();
    let count2 = repo.count_by_session(&session_id2).await.unwrap();

    assert_eq!(count1, 2);
    assert_eq!(count2, 1);
}

// ==================== GET RECENT BY SESSION TESTS ====================

#[tokio::test]
async fn test_get_recent_by_session_returns_limited_messages() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    // Create 5 messages
    for i in 1..=5 {
        let msg = ChatMessage::user_in_session(session_id.clone(), format!("Message {}", i));
        repo.create(msg).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    let result = repo.get_recent_by_session(&session_id, 3).await;

    assert!(result.is_ok());
    let messages = result.unwrap();
    assert_eq!(messages.len(), 3);
}

#[tokio::test]
async fn test_get_recent_by_session_returns_most_recent() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    // Create messages with delays
    for i in 1..=5 {
        let msg = ChatMessage::user_in_session(session_id.clone(), format!("Message {}", i));
        repo.create(msg).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    let messages = repo.get_recent_by_session(&session_id, 2).await.unwrap();

    // Should return the last 2 messages in ascending order
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "Message 4");
    assert_eq!(messages[1].content, "Message 5");
}

#[tokio::test]
async fn test_get_recent_by_session_returns_all_if_fewer_than_limit() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_in_session(session_id.clone(), "Only one");
    repo.create(msg1).await.unwrap();

    let messages = repo.get_recent_by_session(&session_id, 10).await.unwrap();

    assert_eq!(messages.len(), 1);
}

#[tokio::test]
async fn test_get_recent_by_session_returns_in_ascending_order() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_in_session(session_id.clone(), "First");
    repo.create(msg1).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let msg2 = ChatMessage::orchestrator_in_session(session_id.clone(), "Second");
    repo.create(msg2).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let msg3 = ChatMessage::user_in_session(session_id.clone(), "Third");
    repo.create(msg3).await.unwrap();

    let messages = repo.get_recent_by_session(&session_id, 3).await.unwrap();

    // Should be in ascending order (oldest to newest)
    assert_eq!(messages[0].content, "First");
    assert_eq!(messages[1].content, "Second");
    assert_eq!(messages[2].content, "Third");
}

// ==================== SHARED CONNECTION TESTS ====================

#[tokio::test]
async fn test_from_shared_works_correctly() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::from_shared(db.shared_conn());

    let message = ChatMessage::user_in_session(session_id, "Shared connection test");

    let result = repo.create(message.clone()).await;
    assert!(result.is_ok());

    let found = repo.get_by_id(&message.id).await.unwrap();
    assert!(found.is_some());
}

// ==================== ROLE TESTS ====================

#[tokio::test]
async fn test_message_roles_are_preserved() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let user_msg = ChatMessage::user_in_session(session_id.clone(), "User");
    let orch_msg = ChatMessage::orchestrator_in_session(session_id.clone(), "Orchestrator");
    let sys_msg = ChatMessage::system_in_session(session_id.clone(), "System");

    repo.create(user_msg.clone()).await.unwrap();
    repo.create(orch_msg.clone()).await.unwrap();
    repo.create(sys_msg.clone()).await.unwrap();

    let found_user = repo.get_by_id(&user_msg.id).await.unwrap().unwrap();
    let found_orch = repo.get_by_id(&orch_msg.id).await.unwrap().unwrap();
    let found_sys = repo.get_by_id(&sys_msg.id).await.unwrap().unwrap();

    assert!(found_user.is_user());
    assert!(found_orch.is_orchestrator());
    assert!(found_sys.is_system());
}

// ==================== CASCADE DELETE TESTS ====================

#[tokio::test]
async fn test_cascade_delete_when_session_deleted() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let shared_conn = db.shared_conn();
    let repo = SqliteChatMessageRepository::from_shared(shared_conn.clone());

    let msg = ChatMessage::user_in_session(session_id.clone(), "Will be cascaded");
    repo.create(msg.clone()).await.unwrap();

    // Delete the session directly using the shared connection
    {
        let conn = shared_conn.lock().await;
        conn.execute(
            "DELETE FROM ideation_sessions WHERE id = ?1",
            [session_id.as_str()],
        )
        .unwrap();
    }

    // Message should be gone due to CASCADE
    let found = repo.get_by_id(&msg.id).await.unwrap();
    assert!(found.is_none());
}

// ==================== GET_BY_CONVERSATION TESTS ====================

use crate::domain::entities::ChatConversationId;

fn create_test_conversation(db: &SqliteTestDb) -> ChatConversationId {
    let id = ChatConversationId::new();
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (id, context_type, context_id, created_at, updated_at)
             VALUES (?1, 'project', 'test-context', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            [id.as_str()],
        )
        .unwrap();
    });
    id
}

#[tokio::test]
async fn get_by_id_rejects_unknown_non_null_usage_provenance() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let session_id = create_test_session(&db, &project_id);
    let repo = SqliteChatMessageRepository::from_shared(db.shared_conn());
    let message = ChatMessage::orchestrator_in_session(session_id, "usage");
    let message_id = message.id.clone();
    repo.create(message).await.unwrap();
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE chat_messages SET usage_provenance = 'future_capture_kind' WHERE id = ?1",
            [message_id.as_str()],
        )
        .unwrap();
    });

    repo.get_by_id(&message_id)
        .await
        .expect_err("unknown provenance must not be reclassified as legacy data");
}

#[tokio::test]
async fn test_get_by_conversation_returns_messages_in_order() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let conv_id = create_test_conversation(&db);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let mut msg1 = ChatMessage::user_in_project(project_id.clone(), "First message");
    msg1.conversation_id = Some(conv_id.clone());

    let mut msg2 = ChatMessage::user_in_project(project_id.clone(), "Second message");
    msg2.conversation_id = Some(conv_id.clone());

    repo.create(msg1.clone()).await.unwrap();
    repo.create(msg2.clone()).await.unwrap();

    let messages = repo.get_by_conversation(&conv_id).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "First message");
    assert_eq!(messages[1].content, "Second message");
    assert_eq!(messages[0].conversation_id, Some(conv_id.clone()));
}

#[tokio::test]
async fn test_get_by_conversation_returns_empty_for_no_messages() {
    let db = setup_test_db();
    let conv_id = create_test_conversation(&db);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let messages = repo.get_by_conversation(&conv_id).await.unwrap();
    assert!(messages.is_empty());
}

#[tokio::test]
async fn test_get_by_conversation_excludes_other_conversations() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let conv_a = create_test_conversation(&db);
    let conv_b = create_test_conversation(&db);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let mut msg_a = ChatMessage::user_in_project(project_id.clone(), "For A");
    msg_a.conversation_id = Some(conv_a.clone());
    let mut msg_b = ChatMessage::user_in_project(project_id.clone(), "For B");
    msg_b.conversation_id = Some(conv_b.clone());

    repo.create(msg_a).await.unwrap();
    repo.create(msg_b).await.unwrap();

    let messages_a = repo.get_by_conversation(&conv_a).await.unwrap();
    assert_eq!(messages_a.len(), 1);
    assert_eq!(messages_a[0].content, "For A");
}

#[tokio::test]
async fn test_get_recent_by_conversation_paginated_returns_latest_window_in_order() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let conv_id = create_test_conversation(&db);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    for content in ["First", "Second", "Third", "Fourth"] {
        let mut message = ChatMessage::user_in_project(project_id.clone(), content);
        message.conversation_id = Some(conv_id.clone());
        repo.create(message).await.unwrap();
    }

    let latest_window = repo
        .get_recent_by_conversation_paginated(&conv_id, 2, 0)
        .await
        .unwrap();
    assert_eq!(latest_window.len(), 2);
    assert_eq!(latest_window[0].content, "Third");
    assert_eq!(latest_window[1].content, "Fourth");

    let older_window = repo
        .get_recent_by_conversation_paginated(&conv_id, 2, 2)
        .await
        .unwrap();
    assert_eq!(older_window.len(), 2);
    assert_eq!(older_window[0].content, "First");
    assert_eq!(older_window[1].content, "Second");
}

// ==================== ROLE FILTERING TESTS (count_by_session / get_recent_by_session) ====================

#[tokio::test]
async fn test_count_by_session_excludes_system_messages() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let user_msg = ChatMessage::user_in_session(session_id.clone(), "User");
    let orch_msg = ChatMessage::orchestrator_in_session(session_id.clone(), "Orchestrator");
    let sys_msg = ChatMessage::system_in_session(session_id.clone(), "System");

    repo.create(user_msg).await.unwrap();
    repo.create(orch_msg).await.unwrap();
    repo.create(sys_msg).await.unwrap();

    // Should count only user + orchestrator, not system
    let count = repo.count_by_session(&session_id).await.unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_count_by_session_empty_session_returns_zero() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let count = repo.count_by_session(&session_id).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_count_by_session_only_system_messages_returns_zero() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let sys_msg = ChatMessage::system_in_session(session_id.clone(), "System only");
    repo.create(sys_msg).await.unwrap();

    // System messages are excluded — count should be 0
    let count = repo.count_by_session(&session_id).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_get_recent_by_session_excludes_system_messages() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let user_msg = ChatMessage::user_in_session(session_id.clone(), "User message");
    repo.create(user_msg).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let sys_msg = ChatMessage::system_in_session(session_id.clone(), "System message");
    repo.create(sys_msg).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let orch_msg = ChatMessage::orchestrator_in_session(session_id.clone(), "Orchestrator message");
    repo.create(orch_msg).await.unwrap();

    // Should return only user + orchestrator, in chronological order
    let messages = repo.get_recent_by_session(&session_id, 10).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "User message");
    assert_eq!(messages[1].content, "Orchestrator message");
    assert!(messages.iter().all(|m| m.is_user() || m.is_orchestrator()));
}

#[tokio::test]
async fn test_get_recent_by_session_empty_session_returns_empty() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let messages = repo.get_recent_by_session(&session_id, 10).await.unwrap();
    assert!(messages.is_empty());
}

#[tokio::test]
async fn test_get_recent_by_session_limit_enforced_after_role_filter() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    // Create 4 user/orchestrator messages + 2 system messages
    for i in 1..=4 {
        let msg = ChatMessage::user_in_session(session_id.clone(), format!("User {}", i));
        repo.create(msg).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let sys = ChatMessage::system_in_session(session_id.clone(), format!("System {}", i));
        repo.create(sys).await.unwrap();
    }

    // Limit 2 should return the 2 most recent user messages
    let messages = repo.get_recent_by_session(&session_id, 2).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "User 3");
    assert_eq!(messages[1].content, "User 4");
}

#[tokio::test]
async fn test_get_recent_by_session_chronological_order() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test Project", "/test/path");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());

    let msg1 = ChatMessage::user_in_session(session_id.clone(), "First");
    repo.create(msg1).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let msg2 = ChatMessage::orchestrator_in_session(session_id.clone(), "Second");
    repo.create(msg2).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let msg3 = ChatMessage::user_in_session(session_id.clone(), "Third");
    repo.create(msg3).await.unwrap();

    let messages = repo.get_recent_by_session(&session_id, 10).await.unwrap();

    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].content, "First");
    assert_eq!(messages[1].content, "Second");
    assert_eq!(messages[2].content, "Third");
}

// ==================== UPDATE_CONTENT TESTS ====================

#[tokio::test]
async fn test_update_content_changes_content_and_roundtrips() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let msg = ChatMessage::user_in_session(session_id.clone(), "Original content");
    repo.create(msg.clone()).await.unwrap();

    let tool_calls = r#"[{"name":"read_file","input":{"path":"/test"}}]"#;
    let content_blocks = r#"[{"type":"text","text":"Updated"}]"#;

    repo.update_content(
        &msg.id,
        "Updated content",
        Some(tool_calls),
        Some(content_blocks),
    )
    .await
    .unwrap();

    let found = repo.get_by_id(&msg.id).await.unwrap().unwrap();
    assert_eq!(found.content, "Updated content");
    assert_eq!(found.tool_calls, Some(tool_calls.to_string()));
    assert_eq!(found.content_blocks, Some(content_blocks.to_string()));
}

#[tokio::test]
async fn test_update_content_clears_tool_calls_and_content_blocks_when_none() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let session_id = create_test_session(&db, &project_id);

    let repo = SqliteChatMessageRepository::new(db.new_connection());
    let msg = ChatMessage::user_in_session(session_id.clone(), "Original");
    repo.create(msg.clone()).await.unwrap();

    // First set tool_calls + content_blocks
    repo.update_content(&msg.id, "With tools", Some("[]"), Some("[]"))
        .await
        .unwrap();

    // Then clear them
    repo.update_content(&msg.id, "No tools", None, None)
        .await
        .unwrap();

    let found = repo.get_by_id(&msg.id).await.unwrap().unwrap();
    assert_eq!(found.content, "No tools");
    assert!(found.tool_calls.is_none());
    assert!(found.content_blocks.is_none());
}
