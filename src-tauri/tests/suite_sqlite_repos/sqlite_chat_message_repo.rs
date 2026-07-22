use ralphx_lib::domain::entities::{
    AgentRunUsage, ChatConversationId, ChatMessage, ChatMessageId, IdeationSessionId, MessageRole,
    ProviderUsageSnapshot, UsageCapture, UsageProvenance,
};
use ralphx_lib::domain::repositories::ChatMessageRepository;
use ralphx_lib::infrastructure::sqlite::{
    open_connection, run_migrations, SqliteChatMessageRepository,
};

fn standalone_user_message(conversation_id: ChatConversationId, content: &str) -> ChatMessage {
    ChatMessage {
        id: ChatMessageId::new(),
        session_id: None,
        project_id: None,
        task_id: None,
        conversation_id: Some(conversation_id),
        role: MessageRole::User,
        content: content.to_string(),
        metadata: None,
        parent_message_id: None,
        tool_calls: None,
        content_blocks: None,
        attribution_source: None,
        provider_harness: None,
        provider_session_id: None,
        upstream_provider: None,
        provider_profile: None,
        logical_model: None,
        effective_model_id: None,
        logical_effort: None,
        effective_effort: None,
        input_tokens: None,
        output_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        estimated_usd: None,
        usage_provenance: None,
        raw_usage_snapshot: None,
        created_at: chrono::Utc::now(),
    }
}

fn setup_repo() -> SqliteChatMessageRepository {
    let conn = open_connection(&std::path::PathBuf::from(":memory:")).unwrap();
    run_migrations(&conn).unwrap();
    // Disable FK checks so we can insert messages without seeding a parent session row
    conn.execute("PRAGMA foreign_keys = OFF", []).unwrap();
    SqliteChatMessageRepository::new(conn)
}

#[tokio::test]
async fn replace_usage_capture_round_trips_and_can_clear_message_usage() {
    let repo = setup_repo();
    let message = ChatMessage::orchestrator_in_session(IdeationSessionId::new(), "usage");
    let message_id = message.id.clone();
    repo.create(message).await.unwrap();

    repo.replace_usage_capture(
        &message_id,
        &UsageCapture::normalized(
            AgentRunUsage {
                input_tokens: Some(90),
                output_tokens: Some(24),
                cache_creation_tokens: Some(8),
                cache_read_tokens: Some(33),
                estimated_usd: Some(0.015),
            },
            UsageProvenance::ProviderSnapshotFallback,
        ),
    )
    .await
    .unwrap();

    let raw = ProviderUsageSnapshot::from_usage(AgentRunUsage {
        input_tokens: Some(700),
        output_tokens: Some(30),
        cache_creation_tokens: Some(100),
        cache_read_tokens: Some(600),
        estimated_usd: Some(0.04),
    });
    repo.replace_usage_capture(&message_id, &UsageCapture::cumulative_baseline(raw.clone()))
        .await
        .unwrap();

    let retrieved = repo.get_by_id(&message_id).await.unwrap().unwrap();
    assert_eq!(retrieved.input_tokens, None);
    assert_eq!(retrieved.output_tokens, None);
    assert_eq!(
        retrieved.usage_provenance,
        Some(UsageProvenance::CumulativeBaselineOnly)
    );
    assert_eq!(retrieved.raw_usage_snapshot, Some(raw));
}

#[tokio::test]
async fn replace_usage_capture_rejects_missing_message() {
    let repo = setup_repo();

    let error = repo
        .replace_usage_capture(
            &ChatMessageId::new(),
            &UsageCapture::normalized(
                AgentRunUsage {
                    input_tokens: Some(10),
                    ..AgentRunUsage::default()
                },
                UsageProvenance::ProviderTurnDelta,
            ),
        )
        .await
        .expect_err("a missing canonical teammate message must fail closed");

    assert!(matches!(error, ralphx_lib::error::AppError::NotFound(_)));
}

// ==================== GET LATEST MESSAGE BY ROLE TESTS ====================

#[tokio::test]
async fn test_get_latest_message_by_role_returns_none_when_empty() {
    let repo = setup_repo();
    let session_id = IdeationSessionId::new();

    let result = repo
        .get_latest_message_by_role(&session_id, "user")
        .await
        .unwrap();

    assert!(
        result.is_none(),
        "should return None when session has no messages"
    );
}

#[tokio::test]
async fn test_get_latest_message_by_role_returns_none_when_role_not_present() {
    let repo = setup_repo();
    let session_id = IdeationSessionId::new();

    // Insert a user message but query for "orchestrator"
    repo.create(ChatMessage::user_in_session(session_id.clone(), "hello"))
        .await
        .unwrap();

    let result = repo
        .get_latest_message_by_role(&session_id, "orchestrator")
        .await
        .unwrap();

    assert!(
        result.is_none(),
        "should return None when no messages with the requested role exist"
    );
}

#[tokio::test]
async fn test_get_latest_message_by_role_returns_only_message() {
    let repo = setup_repo();
    let session_id = IdeationSessionId::new();

    let msg = repo
        .create(ChatMessage::orchestrator_in_session(
            session_id.clone(),
            "agent reply",
        ))
        .await
        .unwrap();

    let result = repo
        .get_latest_message_by_role(&session_id, "orchestrator")
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "should return the single matching message"
    );
    assert_eq!(result.unwrap().id, msg.id);
}

#[tokio::test]
async fn test_get_latest_message_by_role_returns_correct_latest_with_multiple_messages() {
    let repo = setup_repo();
    let session_id = IdeationSessionId::new();

    // Insert two orchestrator messages with a brief delay so created_at differs
    let _first = repo
        .create(ChatMessage::orchestrator_in_session(
            session_id.clone(),
            "first agent reply",
        ))
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let second = repo
        .create(ChatMessage::orchestrator_in_session(
            session_id.clone(),
            "second agent reply",
        ))
        .await
        .unwrap();

    let result = repo
        .get_latest_message_by_role(&session_id, "orchestrator")
        .await
        .unwrap();

    assert!(result.is_some());
    assert_eq!(
        result.unwrap().id,
        second.id,
        "should return the most recently created message"
    );
}

#[tokio::test]
async fn test_get_latest_message_by_role_filters_by_session() {
    let repo = setup_repo();
    let session_a = IdeationSessionId::new();
    let session_b = IdeationSessionId::new();

    // Insert a message in session_a
    repo.create(ChatMessage::orchestrator_in_session(
        session_a.clone(),
        "msg in a",
    ))
    .await
    .unwrap();

    // Query session_b — should return None
    let result = repo
        .get_latest_message_by_role(&session_b, "orchestrator")
        .await
        .unwrap();

    assert!(
        result.is_none(),
        "should only return messages belonging to the queried session"
    );
}

#[tokio::test]
async fn test_get_latest_message_by_role_does_not_cross_roles() {
    let repo = setup_repo();
    let session_id = IdeationSessionId::new();

    // Insert a user message
    repo.create(ChatMessage::user_in_session(session_id.clone(), "user msg"))
        .await
        .unwrap();

    // Query for "orchestrator" — should return None
    let result = repo
        .get_latest_message_by_role(&session_id, "orchestrator")
        .await
        .unwrap();

    assert!(
        result.is_none(),
        "should not return messages of a different role"
    );
}

// ==================== GET FIRST USER MESSAGE BY CONTEXT (standalone) ====================

#[tokio::test]
async fn test_get_first_user_message_by_context_returns_standalone_message() {
    let repo = setup_repo();
    let conversation_id = ChatConversationId::new();
    repo.create(standalone_user_message(
        conversation_id,
        "First standalone message",
    ))
    .await
    .unwrap();

    let first = repo
        .get_first_user_message_by_context("standalone", &conversation_id.as_str())
        .await
        .unwrap();
    assert_eq!(first.as_deref(), Some("First standalone message"));
}

#[tokio::test]
async fn test_get_first_user_message_by_context_standalone_returns_earliest_message() {
    let repo = setup_repo();
    let conversation_id = ChatConversationId::new();
    let mut earlier = standalone_user_message(conversation_id, "Earlier message");
    earlier.created_at = chrono::Utc::now() - chrono::Duration::seconds(60);
    let later = standalone_user_message(conversation_id, "Later message");
    repo.create(later).await.unwrap();
    repo.create(earlier).await.unwrap();

    let first = repo
        .get_first_user_message_by_context("standalone", &conversation_id.as_str())
        .await
        .unwrap();
    assert_eq!(first.as_deref(), Some("Earlier message"));
}

#[tokio::test]
async fn test_get_first_user_message_by_context_standalone_does_not_leak_other_conversations() {
    let repo = setup_repo();
    let conversation_id = ChatConversationId::new();
    let other_conversation_id = ChatConversationId::new();
    repo.create(standalone_user_message(
        other_conversation_id,
        "Belongs to a different standalone conversation",
    ))
    .await
    .unwrap();

    let first = repo
        .get_first_user_message_by_context("standalone", &conversation_id.as_str())
        .await
        .unwrap();
    assert_eq!(first, None);
}
