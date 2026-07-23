use super::*;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{
    AgentRunUsage, ChatMessageAttribution, ProviderUsageSnapshot, UsageCapture, UsageProvenance,
};

#[tokio::test]
async fn test_create_and_get() {
    let repo = MemoryChatMessageRepository::new();
    let session_id = IdeationSessionId::new();
    let message = ChatMessage::user_in_session(session_id.clone(), "Hello");

    repo.create(message.clone()).await.unwrap();

    let retrieved = repo.get_by_id(&message.id).await.unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().id, message.id);
}

#[tokio::test]
async fn test_get_by_session() {
    let repo = MemoryChatMessageRepository::new();
    let session_id = IdeationSessionId::new();
    let message = ChatMessage::user_in_session(session_id.clone(), "Hello");

    repo.create(message).await.unwrap();

    let messages = repo.get_by_session(&session_id).await.unwrap();
    assert_eq!(messages.len(), 1);
}

#[tokio::test]
async fn test_delete() {
    let repo = MemoryChatMessageRepository::new();
    let session_id = IdeationSessionId::new();
    let message = ChatMessage::user_in_session(session_id.clone(), "Hello");
    let message_id = message.id.clone();

    repo.create(message).await.unwrap();
    repo.delete(&message_id).await.unwrap();

    let result = repo.get_by_id(&message_id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_delete_by_session() {
    let repo = MemoryChatMessageRepository::new();
    let session_id = IdeationSessionId::new();

    repo.create(ChatMessage::user_in_session(session_id.clone(), "Hello 1"))
        .await
        .unwrap();
    repo.create(ChatMessage::user_in_session(session_id.clone(), "Hello 2"))
        .await
        .unwrap();

    repo.delete_by_session(&session_id).await.unwrap();

    let messages = repo.get_by_session(&session_id).await.unwrap();
    assert!(messages.is_empty());
}

#[tokio::test]
async fn test_get_recent_by_session() {
    let repo = MemoryChatMessageRepository::new();
    let session_id = IdeationSessionId::new();

    for i in 1..=5 {
        repo.create(ChatMessage::user_in_session(
            session_id.clone(),
            format!("Message {}", i),
        ))
        .await
        .unwrap();
    }

    let recent = repo.get_recent_by_session(&session_id, 3).await.unwrap();
    assert_eq!(recent.len(), 3);
}

#[tokio::test]
async fn test_update_usage_updates_message_usage_fields() {
    let repo = MemoryChatMessageRepository::new();
    let session_id = IdeationSessionId::new();
    let message = ChatMessage::orchestrator_in_session(session_id, "Usage message");
    let message_id = message.id.clone();

    repo.create(message).await.unwrap();
    repo.update_usage(
        &message_id,
        &AgentRunUsage {
            input_tokens: Some(90),
            output_tokens: Some(24),
            cache_creation_tokens: Some(8),
            cache_read_tokens: Some(33),
            estimated_usd: Some(0.015),
        },
    )
    .await
    .unwrap();

    let updated = repo.get_by_id(&message_id).await.unwrap().unwrap();
    assert_eq!(updated.input_tokens, Some(90));
    assert_eq!(updated.output_tokens, Some(24));
    assert_eq!(updated.cache_creation_tokens, Some(8));
    assert_eq!(updated.cache_read_tokens, Some(33));
    assert_eq!(updated.estimated_usd, Some(0.015));
}

#[tokio::test]
async fn replace_usage_capture_clears_stale_memory_message_usage() {
    let repo = MemoryChatMessageRepository::new();
    let mut message =
        ChatMessage::orchestrator_in_session(IdeationSessionId::new(), "Usage message");
    message.input_tokens = Some(90);
    let message_id = message.id.clone();
    repo.create(message).await.unwrap();
    let raw = ProviderUsageSnapshot::from_usage(AgentRunUsage {
        input_tokens: Some(700),
        ..AgentRunUsage::default()
    });

    repo.replace_usage_capture(&message_id, &UsageCapture::cumulative_baseline(raw.clone()))
        .await
        .unwrap();

    let updated = repo.get_by_id(&message_id).await.unwrap().unwrap();
    assert_eq!(updated.input_tokens, None);
    assert_eq!(updated.raw_usage_snapshot, Some(raw));
    assert_eq!(
        updated.usage_provenance,
        Some(UsageProvenance::CumulativeBaselineOnly)
    );
}

#[tokio::test]
async fn replace_usage_capture_rejects_missing_memory_message() {
    let repo = MemoryChatMessageRepository::new();

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

    assert!(matches!(error, crate::error::AppError::NotFound(_)));
}

#[tokio::test]
async fn test_update_attribution_updates_message_attribution_fields() {
    let repo = MemoryChatMessageRepository::new();
    let session_id = IdeationSessionId::new();
    let message = ChatMessage::orchestrator_in_session(session_id, "Attributed message");
    let message_id = message.id.clone();

    repo.create(message).await.unwrap();
    repo.update_attribution(
        &message_id,
        &ChatMessageAttribution {
            attribution_source: Some("historical_backfill_claude_project_jsonl_z_ai".to_string()),
            provider_harness: Some(AgentHarnessKind::Claude),
            provider_session_id: Some("claude-session-123".to_string()),
            upstream_provider: Some("z_ai".to_string()),
            provider_profile: Some("z_ai".to_string()),
            logical_model: Some("glm-4.7".to_string()),
            effective_model_id: Some("glm-4.7".to_string()),
            logical_effort: Some(LogicalEffort::High),
            effective_effort: Some("high".to_string()),
        },
    )
    .await
    .unwrap();

    let updated = repo.get_by_id(&message_id).await.unwrap().unwrap();
    assert_eq!(
        updated.attribution_source.as_deref(),
        Some("historical_backfill_claude_project_jsonl_z_ai")
    );
    assert_eq!(updated.provider_harness, Some(AgentHarnessKind::Claude));
    assert_eq!(
        updated.provider_session_id.as_deref(),
        Some("claude-session-123")
    );
    assert_eq!(updated.upstream_provider.as_deref(), Some("z_ai"));
    assert_eq!(updated.provider_profile.as_deref(), Some("z_ai"));
    assert_eq!(updated.logical_model.as_deref(), Some("glm-4.7"));
    assert_eq!(updated.effective_model_id.as_deref(), Some("glm-4.7"));
    assert_eq!(updated.logical_effort, Some(LogicalEffort::High));
    assert_eq!(updated.effective_effort.as_deref(), Some("high"));
}

// ── Standalone (self-keyed) context lookups ─────────────────────────────────

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

#[tokio::test]
async fn test_get_first_user_message_by_context_returns_standalone_message() {
    let repo = MemoryChatMessageRepository::new();
    let conversation_id = ChatConversationId::new();
    let message = standalone_user_message(conversation_id, "First standalone message");
    repo.create(message).await.unwrap();

    let first = repo
        .get_first_user_message_by_context("standalone", &conversation_id.as_str())
        .await
        .unwrap();
    assert_eq!(first.as_deref(), Some("First standalone message"));
}

#[tokio::test]
async fn test_get_first_user_message_by_context_standalone_returns_earliest_message() {
    let repo = MemoryChatMessageRepository::new();
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
    let repo = MemoryChatMessageRepository::new();
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
