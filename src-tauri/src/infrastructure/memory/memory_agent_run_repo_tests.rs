use super::*;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{AgentRunAttribution, AgentRunUsage};

#[tokio::test]
async fn test_create_and_get() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let mut run = AgentRun::new(conversation_id);
    run.harness = Some(AgentHarnessKind::Codex);
    run.provider_session_id = Some("session-123".to_string());
    run.logical_effort = Some(LogicalEffort::Medium);
    run.input_tokens = Some(123);
    run.output_tokens = Some(45);
    run.cache_creation_tokens = Some(6);
    run.cache_read_tokens = Some(78);
    run.estimated_usd = Some(0.009);
    let id = run.id;

    repo.create(run.clone()).await.unwrap();

    let retrieved = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(retrieved.id, id);
    assert_eq!(retrieved.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(retrieved.provider_session_id, Some("session-123".to_string()));
    assert_eq!(retrieved.logical_effort, Some(LogicalEffort::Medium));
    assert_eq!(retrieved.input_tokens, Some(123));
    assert_eq!(retrieved.output_tokens, Some(45));
    assert_eq!(retrieved.cache_creation_tokens, Some(6));
    assert_eq!(retrieved.cache_read_tokens, Some(78));
    assert_eq!(retrieved.estimated_usd, Some(0.009));
}

#[tokio::test]
async fn test_get_active_for_conversation() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let run = AgentRun::new(conversation_id);

    repo.create(run.clone()).await.unwrap();

    let active = repo
        .get_active_for_conversation(&conversation_id)
        .await
        .unwrap();
    assert!(active.is_some());
    assert!(active.unwrap().is_active());
}

#[tokio::test]
async fn test_complete() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let run = AgentRun::new(conversation_id);
    let id = run.id;

    repo.create(run).await.unwrap();
    repo.complete(&id).await.unwrap();

    let retrieved = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(retrieved.status, AgentRunStatus::Completed);
    assert!(retrieved.completed_at.is_some());
}

#[tokio::test]
async fn test_update_usage() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let run = AgentRun::new(conversation_id);
    let id = run.id;

    repo.create(run).await.unwrap();
    repo.update_usage(
        &id,
        &AgentRunUsage {
            input_tokens: Some(50),
            output_tokens: Some(20),
            cache_creation_tokens: Some(5),
            cache_read_tokens: Some(10),
            estimated_usd: Some(0.0035),
        },
    )
    .await
    .unwrap();

    let retrieved = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(retrieved.input_tokens, Some(50));
    assert_eq!(retrieved.output_tokens, Some(20));
    assert_eq!(retrieved.cache_creation_tokens, Some(5));
    assert_eq!(retrieved.cache_read_tokens, Some(10));
    assert_eq!(retrieved.estimated_usd, Some(0.0035));
}

#[tokio::test]
async fn test_update_attribution() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let run = AgentRun::new(conversation_id);
    let id = run.id;

    repo.create(run).await.unwrap();
    repo.update_attribution(
        &id,
        &AgentRunAttribution {
            harness: Some(AgentHarnessKind::Claude),
            provider_session_id: Some("claude-session-456".to_string()),
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

    let retrieved = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(retrieved.harness, Some(AgentHarnessKind::Claude));
    assert_eq!(
        retrieved.provider_session_id.as_deref(),
        Some("claude-session-456")
    );
    assert_eq!(retrieved.upstream_provider.as_deref(), Some("z_ai"));
    assert_eq!(retrieved.provider_profile.as_deref(), Some("z_ai"));
    assert_eq!(retrieved.logical_model.as_deref(), Some("glm-4.7"));
    assert_eq!(retrieved.effective_model_id.as_deref(), Some("glm-4.7"));
    assert_eq!(retrieved.logical_effort, Some(LogicalEffort::High));
    assert_eq!(retrieved.effective_effort.as_deref(), Some("high"));
}

#[tokio::test]
async fn test_fail() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let run = AgentRun::new(conversation_id);
    let id = run.id;

    repo.create(run).await.unwrap();
    repo.fail(&id, "Test error").await.unwrap();

    let retrieved = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(retrieved.status, AgentRunStatus::Failed);
    assert_eq!(retrieved.error_message, Some("Test error".to_string()));
}

#[tokio::test]
async fn test_cancel_running_started_before_preserves_current_boot_run() {
    let repo = MemoryAgentRunRepository::new();
    let cutoff = chrono::Utc::now();
    let old_conversation_id = ChatConversationId::new();
    let current_conversation_id = ChatConversationId::new();
    let mut old_run = AgentRun::new(old_conversation_id);
    let mut current_run = AgentRun::new(current_conversation_id);
    old_run.started_at = cutoff - chrono::Duration::seconds(5);
    current_run.started_at = cutoff + chrono::Duration::seconds(5);
    let old_run_id = old_run.id;
    let current_run_id = current_run.id;

    repo.create(old_run).await.unwrap();
    repo.create(current_run).await.unwrap();

    let cancelled = repo.cancel_running_started_before(cutoff).await.unwrap();

    assert_eq!(cancelled, 1);
    let old = repo.get_by_id(&old_run_id).await.unwrap().unwrap();
    assert_eq!(old.status, AgentRunStatus::Cancelled);
    assert_eq!(
        old.error_message,
        Some(ORPHANED_AGENT_RUN_ON_APP_RESTART.to_string())
    );
    let current = repo.get_by_id(&current_run_id).await.unwrap().unwrap();
    assert_eq!(current.status, AgentRunStatus::Running);
    assert_eq!(current.error_message, None);
}
