use super::*;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort, ProviderSessionRef};
use crate::domain::entities::{AgentRunAttribution, AgentRunUsage, IdeationSessionId};
use crate::testing::SqliteTestDb;

fn setup_repo() -> (SqliteTestDb, SqliteAgentRunRepository) {
    let db = SqliteTestDb::new("sqlite-agent-run-repo");
    let repo = SqliteAgentRunRepository::from_shared(db.shared_conn());
    (db, repo)
}

fn seed_ideation_conversation(db: &SqliteTestDb, claude_session_id: Option<&str>) -> ChatConversation {
    let mut conversation = ChatConversation::new_ideation(IdeationSessionId::new());
    conversation.claude_session_id = claude_session_id.map(str::to_string);
    db.insert_conversation(conversation)
}

fn seed_codex_ideation_conversation(db: &SqliteTestDb, provider_session_id: &str) -> ChatConversation {
    let mut conversation = ChatConversation::new_ideation(IdeationSessionId::new());
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: provider_session_id.to_string(),
    });
    db.insert_conversation(conversation)
}

#[tokio::test]
async fn test_get_interrupted_conversations_returns_empty_when_none() {
    let (_db, repo) = setup_repo();

    let result = repo.get_interrupted_conversations().await.unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_get_interrupted_conversations_returns_orphaned_conversation() {
    let (db, agent_run_repo) = setup_repo();
    let conversation = seed_ideation_conversation(&db, Some("test-session-id"));

    // Create an agent run that gets orphaned
    let mut run = AgentRun::new(conversation.id);
    let run_id = run.id;
    run.status = AgentRunStatus::Cancelled;
    run.completed_at = Some(Utc::now());
    run.error_message = Some("Orphaned on app restart".to_string());
    agent_run_repo.create(run).await.unwrap();

    // Get interrupted conversations
    let result = agent_run_repo
        .get_interrupted_conversations()
        .await
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].conversation.id, conversation.id);
    assert_eq!(result[0].last_run.id, run_id);
    assert_eq!(result[0].last_run.status, AgentRunStatus::Cancelled);
    assert_eq!(
        result[0].last_run.error_message,
        Some("Orphaned on app restart".to_string())
    );
}

#[tokio::test]
async fn test_get_interrupted_conversations_returns_orphaned_codex_conversation() {
    let (db, agent_run_repo) = setup_repo();
    let conversation = seed_codex_ideation_conversation(&db, "codex-thread-1");

    let mut run = AgentRun::new(conversation.id);
    let run_id = run.id;
    run.status = AgentRunStatus::Cancelled;
    run.completed_at = Some(Utc::now());
    run.error_message = Some("Orphaned on app restart".to_string());
    run.harness = Some(AgentHarnessKind::Codex);
    run.provider_session_id = Some("codex-thread-1".to_string());
    agent_run_repo.create(run).await.unwrap();

    let result = agent_run_repo
        .get_interrupted_conversations()
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].conversation.id, conversation.id);
    assert_eq!(result[0].conversation.provider_harness, Some(AgentHarnessKind::Codex));
    assert_eq!(
        result[0].conversation.provider_session_id.as_deref(),
        Some("codex-thread-1")
    );
    assert_eq!(result[0].last_run.id, run_id);
    assert_eq!(result[0].last_run.harness, Some(AgentHarnessKind::Codex));
}

#[tokio::test]
async fn test_get_interrupted_conversations_ignores_without_session_id() {
    let (db, agent_run_repo) = setup_repo();
    let conversation = seed_ideation_conversation(&db, None);

    // Create an orphaned agent run
    let mut run = AgentRun::new(conversation.id);
    run.status = AgentRunStatus::Cancelled;
    run.completed_at = Some(Utc::now());
    run.error_message = Some("Orphaned on app restart".to_string());
    agent_run_repo.create(run).await.unwrap();

    // Should return empty because conversation has no claude_session_id
    let result = agent_run_repo
        .get_interrupted_conversations()
        .await
        .unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_get_interrupted_conversations_ignores_completed_runs() {
    let (db, agent_run_repo) = setup_repo();
    let conversation = seed_ideation_conversation(&db, Some("test-session-id"));

    // Create a COMPLETED agent run (not orphaned)
    let mut run = AgentRun::new(conversation.id);
    run.status = AgentRunStatus::Completed;
    run.completed_at = Some(Utc::now());
    agent_run_repo.create(run).await.unwrap();

    // Should return empty because run is completed, not orphaned
    let result = agent_run_repo
        .get_interrupted_conversations()
        .await
        .unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_get_interrupted_conversations_ignores_different_error_message() {
    let (db, agent_run_repo) = setup_repo();
    let conversation = seed_ideation_conversation(&db, Some("test-session-id"));

    // Create a cancelled run with DIFFERENT error message
    let mut run = AgentRun::new(conversation.id);
    run.status = AgentRunStatus::Cancelled;
    run.completed_at = Some(Utc::now());
    run.error_message = Some("User cancelled".to_string());
    agent_run_repo.create(run).await.unwrap();

    // Should return empty because error message doesn't match
    let result = agent_run_repo
        .get_interrupted_conversations()
        .await
        .unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_get_interrupted_conversations_only_latest_run() {
    let (db, agent_run_repo) = setup_repo();
    let conversation = seed_ideation_conversation(&db, Some("test-session-id"));

    // Create an OLD orphaned run
    let mut old_run = AgentRun::new(conversation.id);
    old_run.status = AgentRunStatus::Cancelled;
    old_run.started_at = Utc::now() - chrono::Duration::hours(1);
    old_run.completed_at = Some(Utc::now() - chrono::Duration::hours(1));
    old_run.error_message = Some("Orphaned on app restart".to_string());
    agent_run_repo.create(old_run).await.unwrap();

    // Create a NEW completed run (the latest one)
    let mut new_run = AgentRun::new(conversation.id);
    new_run.status = AgentRunStatus::Completed;
    new_run.started_at = Utc::now();
    new_run.completed_at = Some(Utc::now());
    agent_run_repo.create(new_run).await.unwrap();

    // Should return empty because the LATEST run is completed, not orphaned
    let result = agent_run_repo
        .get_interrupted_conversations()
        .await
        .unwrap();
    assert!(result.is_empty());
}

// ─── create / get_by_id ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_create_and_get_by_id() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    let mut run = AgentRun::new(conv.id);
    run.harness = Some(AgentHarnessKind::Codex);
    run.provider_session_id = Some("session-123".to_string());
    run.logical_model = Some("gpt-5.4".to_string());
    run.effective_model_id = Some("gpt-5.4".to_string());
    run.logical_effort = Some(LogicalEffort::XHigh);
    run.effective_effort = Some("high".to_string());
    run.input_tokens = Some(1200);
    run.output_tokens = Some(450);
    run.cache_creation_tokens = Some(80);
    run.cache_read_tokens = Some(320);
    run.estimated_usd = Some(0.0215);
    run.approval_policy = Some("on-request".to_string());
    run.sandbox_mode = Some("workspace-write".to_string());
    let run_id = run.id;
    repo.create(run).await.unwrap();

    let retrieved = repo.get_by_id(&run_id).await.unwrap();
    assert!(retrieved.is_some());
    let r = retrieved.unwrap();
    assert_eq!(r.id, run_id);
    assert_eq!(r.conversation_id, conv.id);
    assert_eq!(r.status, AgentRunStatus::Running);
    assert_eq!(r.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(r.provider_session_id, Some("session-123".to_string()));
    assert_eq!(r.logical_model, Some("gpt-5.4".to_string()));
    assert_eq!(r.effective_model_id, Some("gpt-5.4".to_string()));
    assert_eq!(r.logical_effort, Some(LogicalEffort::XHigh));
    assert_eq!(r.effective_effort, Some("high".to_string()));
    assert_eq!(r.input_tokens, Some(1200));
    assert_eq!(r.output_tokens, Some(450));
    assert_eq!(r.cache_creation_tokens, Some(80));
    assert_eq!(r.cache_read_tokens, Some(320));
    assert_eq!(r.estimated_usd, Some(0.0215));
    assert_eq!(r.approval_policy, Some("on-request".to_string()));
    assert_eq!(r.sandbox_mode, Some("workspace-write".to_string()));
}

#[tokio::test]
async fn test_update_attribution_updates_agent_run_metadata_fields() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    let run = AgentRun::new(conv.id);
    let run_id = run.id;
    repo.create(run).await.unwrap();

    repo.update_attribution(
        &run_id,
        &AgentRunAttribution {
            harness: Some(AgentHarnessKind::Claude),
            provider_session_id: Some("claude-session-321".to_string()),
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

    let retrieved = repo.get_by_id(&run_id).await.unwrap().unwrap();
    assert_eq!(retrieved.harness, Some(AgentHarnessKind::Claude));
    assert_eq!(
        retrieved.provider_session_id.as_deref(),
        Some("claude-session-321")
    );
    assert_eq!(retrieved.upstream_provider.as_deref(), Some("z_ai"));
    assert_eq!(retrieved.provider_profile.as_deref(), Some("z_ai"));
    assert_eq!(retrieved.logical_model.as_deref(), Some("glm-4.7"));
    assert_eq!(retrieved.effective_model_id.as_deref(), Some("glm-4.7"));
    assert_eq!(retrieved.logical_effort, Some(LogicalEffort::High));
    assert_eq!(retrieved.effective_effort.as_deref(), Some("high"));
}

#[tokio::test]
async fn test_get_by_id_not_found() {
    let (_db, repo) = setup_repo();

    let fake_id = AgentRunId::from_string("nonexistent-id".to_string());
    assert!(repo.get_by_id(&fake_id).await.unwrap().is_none());
}

#[tokio::test]
async fn test_update_usage_persists_fields() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    let run = AgentRun::new(conv.id);
    let run_id = run.id;
    repo.create(run).await.unwrap();

    repo.update_usage(
        &run_id,
        &AgentRunUsage {
            input_tokens: Some(77),
            output_tokens: Some(31),
            cache_creation_tokens: Some(9),
            cache_read_tokens: Some(18),
            estimated_usd: Some(0.0042),
        },
    )
    .await
    .unwrap();

    let retrieved = repo.get_by_id(&run_id).await.unwrap().unwrap();
    assert_eq!(retrieved.input_tokens, Some(77));
    assert_eq!(retrieved.output_tokens, Some(31));
    assert_eq!(retrieved.cache_creation_tokens, Some(9));
    assert_eq!(retrieved.cache_read_tokens, Some(18));
    assert_eq!(retrieved.estimated_usd, Some(0.0042));
}

// ─── get_latest / get_active ─────────────────────────────────────────────────

#[tokio::test]
async fn test_get_latest_for_conversation() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    let mut old_run = AgentRun::new(conv.id);
    old_run.started_at = Utc::now() - chrono::Duration::hours(1);
    repo.create(old_run).await.unwrap();

    let new_run = AgentRun::new(conv.id);
    let new_run_id = new_run.id;
    repo.create(new_run).await.unwrap();

    let latest = repo.get_latest_for_conversation(&conv.id).await.unwrap();
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().id, new_run_id);
}

#[tokio::test]
async fn test_get_latest_for_conversation_empty() {
    let (_db, repo) = setup_repo();

    let fake_id = ChatConversationId::from_string("no-such-conv".to_string());
    assert!(repo.get_latest_for_conversation(&fake_id).await.unwrap().is_none());
}

#[tokio::test]
async fn test_get_active_for_conversation() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    // No active run yet
    assert!(repo.get_active_for_conversation(&conv.id).await.unwrap().is_none());

    let run = AgentRun::new(conv.id);
    let run_id = run.id;
    repo.create(run).await.unwrap();

    let active = repo.get_active_for_conversation(&conv.id).await.unwrap();
    assert!(active.is_some());
    assert_eq!(active.unwrap().id, run_id);
}

#[tokio::test]
async fn test_get_active_for_conversation_excludes_terminal_runs() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    let mut run = AgentRun::new(conv.id);
    run.status = AgentRunStatus::Completed;
    run.completed_at = Some(Utc::now());
    repo.create(run).await.unwrap();

    assert!(repo.get_active_for_conversation(&conv.id).await.unwrap().is_none());
}

// ─── get_by_conversation ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_by_conversation() {
    let (db, repo) = setup_repo();
    let conv1 = db.seed_ideation_conversation();
    let conv2 = db.seed_ideation_conversation();

    let mut r1 = AgentRun::new(conv1.id);
    r1.started_at = Utc::now() - chrono::Duration::hours(2);
    let mut r2 = AgentRun::new(conv1.id);
    r2.started_at = Utc::now() - chrono::Duration::hours(1);
    let r3 = AgentRun::new(conv2.id);

    repo.create(r1).await.unwrap();
    repo.create(r2).await.unwrap();
    repo.create(r3).await.unwrap();

    assert_eq!(repo.get_by_conversation(&conv1.id).await.unwrap().len(), 2);
    assert_eq!(repo.get_by_conversation(&conv2.id).await.unwrap().len(), 1);
}

// ─── update_status / complete / fail / cancel ────────────────────────────────

#[tokio::test]
async fn test_update_status() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    let run = AgentRun::new(conv.id);
    let run_id = run.id;
    repo.create(run).await.unwrap();

    repo.update_status(&run_id, AgentRunStatus::Cancelled).await.unwrap();

    let updated = repo.get_by_id(&run_id).await.unwrap().unwrap();
    assert_eq!(updated.status, AgentRunStatus::Cancelled);
}

#[tokio::test]
async fn test_complete() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    let run = AgentRun::new(conv.id);
    let run_id = run.id;
    repo.create(run).await.unwrap();

    repo.complete(&run_id).await.unwrap();

    let updated = repo.get_by_id(&run_id).await.unwrap().unwrap();
    assert_eq!(updated.status, AgentRunStatus::Completed);
    assert!(updated.completed_at.is_some());
    assert!(updated.error_message.is_none());
}

#[tokio::test]
async fn test_fail() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    let run = AgentRun::new(conv.id);
    let run_id = run.id;
    repo.create(run).await.unwrap();

    repo.fail(&run_id, "Something went wrong").await.unwrap();

    let updated = repo.get_by_id(&run_id).await.unwrap().unwrap();
    assert_eq!(updated.status, AgentRunStatus::Failed);
    assert!(updated.completed_at.is_some());
    assert_eq!(updated.error_message, Some("Something went wrong".to_string()));
}

#[tokio::test]
async fn test_cancel() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    let run = AgentRun::new(conv.id);
    let run_id = run.id;
    repo.create(run).await.unwrap();

    repo.cancel(&run_id).await.unwrap();

    let updated = repo.get_by_id(&run_id).await.unwrap().unwrap();
    assert_eq!(updated.status, AgentRunStatus::Cancelled);
    assert!(updated.completed_at.is_some());
    assert!(updated.error_message.is_none());
}

// ─── delete ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_delete() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    let run = AgentRun::new(conv.id);
    let run_id = run.id;
    repo.create(run).await.unwrap();

    assert!(repo.get_by_id(&run_id).await.unwrap().is_some());

    repo.delete(&run_id).await.unwrap();

    assert!(repo.get_by_id(&run_id).await.unwrap().is_none());
}

#[tokio::test]
async fn test_delete_by_conversation() {
    let (db, repo) = setup_repo();
    let conv1 = db.seed_ideation_conversation();
    let conv2 = db.seed_ideation_conversation();

    repo.create(AgentRun::new(conv1.id)).await.unwrap();
    repo.create(AgentRun::new(conv1.id)).await.unwrap();
    let run2 = AgentRun::new(conv2.id);
    let run2_id = run2.id;
    repo.create(run2).await.unwrap();

    repo.delete_by_conversation(&conv1.id).await.unwrap();

    assert!(repo.get_by_conversation(&conv1.id).await.unwrap().is_empty());
    assert!(repo.get_by_id(&run2_id).await.unwrap().is_some());
}

// ─── count_by_status ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_count_by_status() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    let r1 = AgentRun::new(conv.id);
    let r2 = AgentRun::new(conv.id);
    let r3 = AgentRun::new(conv.id);
    let r3_id = r3.id;
    repo.create(r1).await.unwrap();
    repo.create(r2).await.unwrap();
    repo.create(r3).await.unwrap();

    repo.cancel(&r3_id).await.unwrap();

    assert_eq!(repo.count_by_status(&conv.id, AgentRunStatus::Running).await.unwrap(), 2);
    assert_eq!(repo.count_by_status(&conv.id, AgentRunStatus::Cancelled).await.unwrap(), 1);
    assert_eq!(repo.count_by_status(&conv.id, AgentRunStatus::Completed).await.unwrap(), 0);
}

// ─── cancel_all_running ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_cancel_all_running() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();

    let r1 = AgentRun::new(conv.id);
    let r2 = AgentRun::new(conv.id);
    let r3 = AgentRun::new(conv.id);
    let r1_id = r1.id;
    let r2_id = r2.id;
    let r3_id = r3.id;
    repo.create(r1).await.unwrap();
    repo.create(r2).await.unwrap();
    repo.create(r3).await.unwrap();

    // Complete r3 before cancel_all_running
    repo.complete(&r3_id).await.unwrap();

    let cancelled_count = repo.cancel_all_running().await.unwrap();
    assert_eq!(cancelled_count, 2);

    let r1u = repo.get_by_id(&r1_id).await.unwrap().unwrap();
    assert_eq!(r1u.status, AgentRunStatus::Cancelled);
    assert_eq!(r1u.error_message, Some("Orphaned on app restart".to_string()));

    let r2u = repo.get_by_id(&r2_id).await.unwrap().unwrap();
    assert_eq!(r2u.status, AgentRunStatus::Cancelled);

    // Completed run must not be affected
    let r3u = repo.get_by_id(&r3_id).await.unwrap().unwrap();
    assert_eq!(r3u.status, AgentRunStatus::Completed);
}

#[tokio::test]
async fn test_cancel_running_started_before_preserves_current_boot_run() {
    let (db, repo) = setup_repo();
    let conv = db.seed_ideation_conversation();
    let cutoff = Utc::now();
    let mut old_run = AgentRun::new(conv.id);
    let mut current_run = AgentRun::new(conv.id);
    old_run.started_at = cutoff - chrono::Duration::seconds(5);
    current_run.started_at = cutoff + chrono::Duration::seconds(5);
    let old_run_id = old_run.id;
    let current_run_id = current_run.id;

    repo.create(old_run).await.unwrap();
    repo.create(current_run).await.unwrap();

    let cancelled_count = repo.cancel_running_started_before(cutoff).await.unwrap();

    assert_eq!(cancelled_count, 1);
    let old = repo.get_by_id(&old_run_id).await.unwrap().unwrap();
    assert_eq!(old.status, AgentRunStatus::Cancelled);
    assert_eq!(
        old.error_message,
        Some("Orphaned on app restart".to_string())
    );

    let current = repo.get_by_id(&current_run_id).await.unwrap().unwrap();
    assert_eq!(current.status, AgentRunStatus::Running);
    assert_eq!(current.error_message, None);
}
