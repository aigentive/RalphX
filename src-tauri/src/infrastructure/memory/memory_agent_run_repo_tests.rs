use super::*;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::agent_run::PersonaRunAttribution;
use crate::domain::entities::{
    AgentRunAttribution, AgentRunUsage, ProviderUsageSnapshot, RuntimeSource, UsageCapture,
    UsageProvenance,
};
use crate::domain::repositories::{ORPHANED_AGENT_RUN_ON_APP_RESTART, PRUNED_STALE_AGENT_RUN};

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
    assert_eq!(
        retrieved.provider_session_id,
        Some("session-123".to_string())
    );
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
async fn latest_completed_provider_session_ignores_newer_failed_and_foreign_runs() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();
    let mut owning_run = AgentRun::new(conversation_id);
    owning_run.status = AgentRunStatus::Completed;
    owning_run.started_at = chrono::Utc::now() - chrono::Duration::minutes(4);
    owning_run.harness = Some(AgentHarnessKind::Codex);
    owning_run.provider_session_id = Some("codex-session".to_string());
    owning_run.effective_model_id = Some("gpt-5.6-sol".to_string());
    let owning_id = owning_run.id;
    repo.create(owning_run).await.unwrap();

    let mut failed = AgentRun::new(conversation_id);
    failed.status = AgentRunStatus::Failed;
    failed.started_at = chrono::Utc::now();
    failed.harness = Some(AgentHarnessKind::Codex);
    repo.create(failed).await.unwrap();

    let mut foreign = AgentRun::new(conversation_id);
    foreign.status = AgentRunStatus::Completed;
    foreign.started_at = chrono::Utc::now() - chrono::Duration::minutes(1);
    foreign.harness = Some(AgentHarnessKind::Claude);
    foreign.provider_session_id = Some("codex-session".to_string());
    repo.create(foreign).await.unwrap();

    let found = repo
        .get_latest_completed_for_provider_session(
            &conversation_id,
            AgentHarnessKind::Codex,
            "codex-session",
        )
        .await
        .unwrap()
        .expect("owning completed provider run");

    assert_eq!(found.id, owning_id);
    assert_eq!(found.effective_model_id.as_deref(), Some("gpt-5.6-sol"));
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
async fn complete_if_running_is_compare_and_set() {
    let repo = MemoryAgentRunRepository::new();
    let running = AgentRun::new(ChatConversationId::new());
    let running_id = running.id;
    repo.create(running).await.unwrap();

    assert!(repo.complete_if_running(&running_id).await.unwrap());
    assert!(!repo.complete_if_running(&running_id).await.unwrap());

    let mut failed = AgentRun::new(ChatConversationId::new());
    failed.status = AgentRunStatus::Failed;
    let failed_id = failed.id;
    repo.create(failed).await.unwrap();
    assert!(!repo.complete_if_running(&failed_id).await.unwrap());
    assert_eq!(
        repo.get_by_id(&failed_id).await.unwrap().unwrap().status,
        AgentRunStatus::Failed
    );
    assert!(!repo.complete_if_running(&AgentRunId::new()).await.unwrap());
}

#[tokio::test]
async fn prune_cancel_repair_is_attributed_idempotent_and_fail_closed() {
    let repo = MemoryAgentRunRepository::new();
    let conversation_id = ChatConversationId::new();

    let marked = AgentRun::new(conversation_id.clone());
    let marked_id = marked.id;
    repo.create(marked).await.unwrap();
    repo.cancel_with_reason(&marked_id, PRUNED_STALE_AGENT_RUN)
        .await
        .unwrap();
    let marked_cancel = repo.get_by_id(&marked_id).await.unwrap().unwrap();
    assert_eq!(marked_cancel.status, AgentRunStatus::Cancelled);
    assert_eq!(
        marked_cancel.error_message.as_deref(),
        Some(PRUNED_STALE_AGENT_RUN)
    );
    assert!(repo.complete_if_prune_cancelled(&marked_id).await.unwrap());
    assert!(!repo.complete_if_prune_cancelled(&marked_id).await.unwrap());

    let user_cancelled = AgentRun::new(conversation_id.clone());
    let user_cancelled_id = user_cancelled.id;
    repo.create(user_cancelled).await.unwrap();
    repo.cancel(&user_cancelled_id).await.unwrap();

    let mut failed = AgentRun::new(conversation_id.clone());
    failed.fail("provider failed");
    let failed_id = failed.id;
    repo.create(failed).await.unwrap();

    let mut completed = AgentRun::new(conversation_id.clone());
    completed.complete();
    let completed_id = completed.id;
    repo.create(completed).await.unwrap();
    repo.cancel_with_reason(&completed_id, PRUNED_STALE_AGENT_RUN)
        .await
        .unwrap();

    repo.cancel_with_reason(&failed_id, PRUNED_STALE_AGENT_RUN)
        .await
        .unwrap();

    let mut restart_orphan = AgentRun::new(conversation_id);
    restart_orphan.status = AgentRunStatus::Cancelled;
    restart_orphan.error_message = Some(ORPHANED_AGENT_RUN_ON_APP_RESTART.to_string());
    let restart_orphan_id = restart_orphan.id;
    repo.create(restart_orphan).await.unwrap();

    for id in [
        &user_cancelled_id,
        &failed_id,
        &completed_id,
        &restart_orphan_id,
    ] {
        assert!(!repo.complete_if_prune_cancelled(id).await.unwrap());
    }
    assert!(!repo
        .complete_if_prune_cancelled(&AgentRunId::new())
        .await
        .unwrap());

    assert_eq!(
        repo.get_by_id(&user_cancelled_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentRunStatus::Cancelled
    );
    assert_eq!(
        repo.get_by_id(&failed_id).await.unwrap().unwrap().status,
        AgentRunStatus::Failed
    );
    let completed = repo.get_by_id(&completed_id).await.unwrap().unwrap();
    assert_eq!(completed.status, AgentRunStatus::Completed);
    assert_eq!(completed.error_message, None);
    assert_eq!(
        repo.get_by_id(&restart_orphan_id)
            .await
            .unwrap()
            .unwrap()
            .error_message
            .as_deref(),
        Some(ORPHANED_AGENT_RUN_ON_APP_RESTART)
    );
}

#[tokio::test]
async fn active_action_is_scoped_to_owning_conversation() {
    let repo = MemoryAgentRunRepository::new();
    let owner = ChatConversationId::new();
    let detached = ChatConversationId::new();
    let mut owner_run = AgentRun::new(owner);
    owner_run.action_kind = Some(AgentRunActionKind::VerifyPlan);
    owner_run.action_context_id = Some("session-1".to_string());
    owner_run.action_target_id = Some("artifact-1".to_string());
    let owner_id = owner_run.id;
    repo.create(owner_run).await.unwrap();

    let mut detached_run = AgentRun::new(detached);
    detached_run.action_kind = Some(AgentRunActionKind::VerifyPlan);
    detached_run.action_context_id = Some("session-1".to_string());
    detached_run.action_target_id = Some("artifact-1".to_string());
    detached_run.started_at = chrono::Utc::now() + chrono::Duration::seconds(1);
    repo.create(detached_run).await.unwrap();

    let found = repo
        .get_active_action(
            &owner,
            AgentRunActionKind::VerifyPlan,
            "session-1",
            "artifact-1",
        )
        .await
        .unwrap()
        .expect("owner action");
    assert_eq!(found.id, owner_id);

    let latest = repo
        .get_latest_action(
            &owner,
            AgentRunActionKind::VerifyPlan,
            "session-1",
            "artifact-1",
        )
        .await
        .unwrap()
        .expect("latest owner action");
    assert_eq!(latest.id, owner_id);
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
async fn replace_usage_capture_clears_stale_memory_run_usage() {
    let repo = MemoryAgentRunRepository::new();
    let mut run = AgentRun::new(ChatConversationId::new());
    run.input_tokens = Some(100);
    let id = run.id;
    repo.create(run).await.unwrap();
    let raw = ProviderUsageSnapshot::from_usage(AgentRunUsage {
        input_tokens: Some(500),
        ..AgentRunUsage::default()
    });

    repo.replace_usage_capture(&id, &UsageCapture::cumulative_baseline(raw.clone()))
        .await
        .unwrap();

    let retrieved = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(retrieved.input_tokens, None);
    assert_eq!(retrieved.raw_usage_snapshot, Some(raw));
    assert_eq!(
        retrieved.usage_provenance,
        Some(UsageProvenance::CumulativeBaselineOnly)
    );
}

#[tokio::test]
async fn replace_usage_capture_rejects_missing_memory_run() {
    let repo = MemoryAgentRunRepository::new();
    let missing_id = AgentRunId::new();

    let error = repo
        .replace_usage_capture(
            &missing_id,
            &UsageCapture::normalized(
                AgentRunUsage {
                    input_tokens: Some(10),
                    ..AgentRunUsage::default()
                },
                UsageProvenance::ProviderTurnDelta,
            ),
        )
        .await
        .expect_err("a missing canonical run must fail closed");

    assert!(matches!(error, crate::error::AppError::NotFound(_)));
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
            service_tier: Some("fast".to_string()),
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
    assert_eq!(retrieved.service_tier.as_deref(), Some("fast"));
}

#[tokio::test]
async fn agent_run_identity_fields_round_trip_in_memory_repo() {
    let repo = MemoryAgentRunRepository::new();
    let mut run = AgentRun::new(ChatConversationId::new());
    let run_id = run.id;
    run.agent_name = Some("ralphx-workspace-reviewer".to_string());
    run.launch_role = Some("workspace_reviewer".to_string());
    run.runtime_source = Some(RuntimeSource::RoleDefault);

    repo.create(run).await.unwrap();

    let persisted = repo.get_by_id(&run_id).await.unwrap().unwrap();
    assert_eq!(
        persisted.agent_name.as_deref(),
        Some("ralphx-workspace-reviewer")
    );
    assert_eq!(persisted.launch_role.as_deref(), Some("workspace_reviewer"));
    assert_eq!(persisted.runtime_source, Some(RuntimeSource::RoleDefault));
}

#[tokio::test]
async fn get_by_ids_returns_only_requested_memory_runs() {
    let repo = MemoryAgentRunRepository::new();
    let first = AgentRun::new(ChatConversationId::new());
    let first_id = first.id;
    let second = AgentRun::new(ChatConversationId::new());
    let second_id = second.id;
    let omitted = AgentRun::new(ChatConversationId::new());
    let omitted_id = omitted.id;
    repo.create(first).await.unwrap();
    repo.create(second).await.unwrap();
    repo.create(omitted).await.unwrap();

    let runs = repo
        .get_by_ids(&[second_id, AgentRunId::new(), first_id])
        .await
        .unwrap();
    let ids = runs
        .iter()
        .map(|run| run.id)
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(runs.len(), 2);
    assert!(ids.contains(&first_id));
    assert!(ids.contains(&second_id));
    assert!(!ids.contains(&omitted_id));
}

#[tokio::test]
async fn update_status_sets_terminal_timestamp_once_and_clears_it_for_running() {
    let repo = MemoryAgentRunRepository::new();
    let fixed_completed_at = chrono::Utc::now() - chrono::Duration::minutes(1);

    for status in [
        AgentRunStatus::Completed,
        AgentRunStatus::Failed,
        AgentRunStatus::Cancelled,
    ] {
        let run = AgentRun::new(ChatConversationId::new());
        let run_id = run.id;
        repo.create(run).await.unwrap();
        repo.update_status(&run_id, status).await.unwrap();
        assert!(repo
            .get_by_id(&run_id)
            .await
            .unwrap()
            .expect("persisted run")
            .completed_at
            .is_some());
    }

    let mut preserved = AgentRun::new(ChatConversationId::new());
    let preserved_id = preserved.id;
    preserved.completed_at = Some(fixed_completed_at);
    repo.create(preserved).await.unwrap();
    repo.update_status(&preserved_id, AgentRunStatus::Cancelled)
        .await
        .unwrap();
    assert_eq!(
        repo.get_by_id(&preserved_id)
            .await
            .unwrap()
            .expect("persisted run")
            .completed_at,
        Some(fixed_completed_at)
    );

    repo.update_status(&preserved_id, AgentRunStatus::Running)
        .await
        .unwrap();
    assert!(repo
        .get_by_id(&preserved_id)
        .await
        .unwrap()
        .expect("persisted run")
        .completed_at
        .is_none());
}

#[tokio::test]
async fn persona_run_attribution_round_trips_in_memory_repo() {
    let repo = MemoryAgentRunRepository::new();
    let run = AgentRun::new(ChatConversationId::new());
    let run_id = run.id;
    repo.create(run).await.unwrap();

    repo.set_persona_attribution(
        &run_id,
        PersonaRunAttribution {
            persona_id: "persona-1".to_string(),
            persona_slug: "design-voice".to_string(),
            persona_version: 2,
            persona_content_hash: "content-hash".to_string(),
            injected: false,
            skipped_reason: Some("native_agent_flag".to_string()),
        },
    )
    .await
    .unwrap();

    let persisted = repo.get_by_id(&run_id).await.unwrap().unwrap();
    assert_eq!(persisted.persona_slug.as_deref(), Some("design-voice"));
    assert_eq!(persisted.persona_version, Some(2));
    assert_eq!(persisted.persona_injected, Some(false));
    assert_eq!(
        persisted.persona_skipped_reason.as_deref(),
        Some("native_agent_flag")
    );
}

#[tokio::test]
async fn persona_run_attribution_defaults_to_null_in_memory_repo() {
    let repo = MemoryAgentRunRepository::new();
    let run = AgentRun::new(ChatConversationId::new());
    let run_id = run.id;
    repo.create(run).await.unwrap();

    let persisted = repo.get_by_id(&run_id).await.unwrap().unwrap();
    assert!(persisted.persona_id.is_none());
    assert!(persisted.persona_injected.is_none());
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
