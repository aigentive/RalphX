use super::*;
use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AttributionBackfillStatus, CoordinationMode, IdeationSessionId,
    ProjectId,
};

#[tokio::test]
async fn test_create_and_get() {
    let repo = MemoryChatConversationRepository::new();
    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id);
    let id = conv.id;

    repo.create(conv.clone()).await.unwrap();

    let retrieved = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(retrieved.id, id);
}

#[tokio::test]
async fn create_accepts_valid_standalone_self_key() {
    let repo = MemoryChatConversationRepository::new();
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
    let repo = MemoryChatConversationRepository::new();
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
async fn refresh_provider_session_ref_updates_existing_and_skips_cleared() {
    let repo = MemoryChatConversationRepository::new();
    let conversation =
        ChatConversation::new_project(ProjectId::from_string("project-refresh".to_string()));
    repo.create(conversation.clone()).await.unwrap();

    repo.update_provider_session_ref(
        &conversation.id,
        &ProviderSessionRef {
            harness: AgentHarnessKind::Claude,
            provider_session_id: "session-1".to_string(),
        },
    )
    .await
    .unwrap();

    let refreshed = repo
        .refresh_provider_session_ref(
            &conversation.id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: "session-2".to_string(),
            },
        )
        .await
        .unwrap();
    assert!(refreshed);
    assert_eq!(
        repo.get_by_id(&conversation.id)
            .await
            .unwrap()
            .unwrap()
            .provider_session_ref()
            .map(|session_ref| session_ref.provider_session_id),
        Some("session-2".to_string())
    );

    repo.clear_provider_session_ref(&conversation.id)
        .await
        .unwrap();

    let refreshed_after_clear = repo
        .refresh_provider_session_ref(
            &conversation.id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: "session-3".to_string(),
            },
        )
        .await
        .unwrap();
    assert!(
        !refreshed_after_clear,
        "cleared ref must not be resurrected by a late teardown write"
    );
    assert!(repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap()
        .provider_session_ref()
        .is_none());
}

#[tokio::test]
async fn test_update_builder_draft_binding_sets_and_clears() {
    let repo = MemoryChatConversationRepository::new();
    let conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
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
async fn update_agent_mode_and_role_bindings_persists_one_tuple() {
    let repo = MemoryChatConversationRepository::new();
    let conversation =
        ChatConversation::new_project(ProjectId::from_string("project-atomic-edit".to_string()));
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

#[tokio::test]
async fn test_get_by_builder_draft_id_returns_newest_active_binding() {
    let repo = MemoryChatConversationRepository::new();
    let project = ProjectId::from_string("project-1".to_string());
    let mut older = ChatConversation::new_project(project.clone());
    older.builder_draft_id = Some("draft-1".to_string());
    older.created_at = chrono::Utc::now() - chrono::Duration::minutes(2);
    let mut newest = ChatConversation::new_project(project.clone());
    newest.builder_draft_id = Some("draft-1".to_string());
    newest.created_at = chrono::Utc::now() - chrono::Duration::minutes(1);
    let mut archived = ChatConversation::new_project(project);
    archived.builder_draft_id = Some("draft-1".to_string());
    archived.archived_at = Some(chrono::Utc::now());
    let mut other_draft =
        ChatConversation::new_project(ProjectId::from_string("project-2".to_string()));
    other_draft.builder_draft_id = Some("draft-2".to_string());

    repo.create(older).await.unwrap();
    repo.create(newest.clone()).await.unwrap();
    repo.create(archived).await.unwrap();
    repo.create(other_draft).await.unwrap();

    assert_eq!(
        repo.get_by_builder_draft_id("draft-1")
            .await
            .unwrap()
            .unwrap()
            .id,
        newest.id
    );
    assert!(repo
        .get_by_builder_draft_id("missing-draft")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_get_by_context() {
    let repo = MemoryChatConversationRepository::new();
    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id.clone());

    repo.create(conv.clone()).await.unwrap();

    let convos = repo
        .get_by_context(ChatContextType::Ideation, session_id.as_str())
        .await
        .unwrap();
    assert_eq!(convos.len(), 1);
}

#[tokio::test]
async fn test_get_by_context_page_filtered_can_return_archived_only() {
    let repo = MemoryChatConversationRepository::new();

    let mut active = ChatConversation::new_project(
        crate::domain::entities::ProjectId::from_string("project-1".to_string()),
    );
    active.title = Some("Active agent".to_string());

    let mut archived = ChatConversation::new_project(
        crate::domain::entities::ProjectId::from_string("project-1".to_string()),
    );
    archived.title = Some("Archived agent".to_string());
    archived.archived_at = Some(chrono::Utc::now());

    repo.create(active.clone()).await.unwrap();
    repo.create(archived.clone()).await.unwrap();

    let page = repo
        .get_by_context_page_filtered(
            ChatContextType::Project,
            "project-1",
            true,
            true,
            0,
            10,
            None,
        )
        .await
        .unwrap();

    assert_eq!(page.total_count, 1);
    assert_eq!(page.conversations.len(), 1);
    assert_eq!(page.conversations[0].id, archived.id);
}

#[tokio::test]
async fn test_list_recent_resumable_by_context_type_filters_and_orders() {
    let repo = MemoryChatConversationRepository::new();
    let now = chrono::Utc::now();

    let mut oldest = ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    oldest.provider_session_id = Some("codex-oldest".to_string());
    oldest.provider_harness = Some(AgentHarnessKind::Codex);
    oldest.last_message_at = Some(now - chrono::Duration::minutes(5));
    oldest.updated_at = now - chrono::Duration::minutes(1);

    let mut newest = ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    newest.claude_session_id = Some("claude-newest".to_string());
    newest.last_message_at = Some(now - chrono::Duration::minutes(1));
    newest.updated_at = now - chrono::Duration::minutes(5);

    let mut middle = ChatConversation::new_project(ProjectId::from_string("project-2".to_string()));
    middle.provider_session_id = Some("codex-middle".to_string());
    middle.provider_harness = Some(AgentHarnessKind::Codex);
    middle.last_message_at = Some(now - chrono::Duration::minutes(3));

    let missing_session =
        ChatConversation::new_project(ProjectId::from_string("project-3".to_string()));

    let mut archived =
        ChatConversation::new_project(ProjectId::from_string("project-4".to_string()));
    archived.provider_session_id = Some("codex-archived".to_string());
    archived.provider_harness = Some(AgentHarnessKind::Codex);
    archived.archived_at = Some(now);

    let mut ideation = ChatConversation::new_ideation(IdeationSessionId::new());
    ideation.provider_session_id = Some("codex-ideation".to_string());
    ideation.provider_harness = Some(AgentHarnessKind::Codex);

    repo.create(oldest.clone()).await.unwrap();
    repo.create(newest.clone()).await.unwrap();
    repo.create(middle.clone()).await.unwrap();
    repo.create(missing_session).await.unwrap();
    repo.create(archived).await.unwrap();
    repo.create(ideation).await.unwrap();

    let resumable = repo
        .list_recent_resumable_by_context_type(ChatContextType::Project, 2)
        .await
        .unwrap();

    assert_eq!(
        resumable
            .iter()
            .map(|conversation| conversation.id)
            .collect::<Vec<_>>(),
        vec![newest.id, middle.id]
    );
}

#[tokio::test]
async fn test_update_claude_session_id() {
    let repo = MemoryChatConversationRepository::new();
    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id);
    let id = conv.id;

    repo.create(conv).await.unwrap();
    repo.update_claude_session_id(&id, "test-session-123")
        .await
        .unwrap();

    let retrieved = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(
        retrieved.claude_session_id,
        Some("test-session-123".to_string())
    );
    assert_eq!(
        retrieved.provider_session_id,
        Some("test-session-123".to_string())
    );
    assert_eq!(retrieved.provider_harness, Some(AgentHarnessKind::Claude));
}

#[tokio::test]
async fn test_update_provider_session_ref_for_codex() {
    let repo = MemoryChatConversationRepository::new();
    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id);
    let id = conv.id;

    repo.create(conv).await.unwrap();
    repo.update_provider_session_ref(
        &id,
        &ProviderSessionRef {
            harness: AgentHarnessKind::Codex,
            provider_session_id: "codex-session-1".to_string(),
        },
    )
    .await
    .unwrap();

    let retrieved = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(retrieved.provider_harness, Some(AgentHarnessKind::Codex));
    assert_eq!(
        retrieved.provider_session_id,
        Some("codex-session-1".to_string())
    );
    assert_eq!(retrieved.claude_session_id, None);
}

#[tokio::test]
async fn test_update_provider_origin() {
    let repo = MemoryChatConversationRepository::new();
    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id);
    let id = conv.id;

    repo.create(conv).await.unwrap();
    repo.update_provider_origin(&id, Some("z_ai"), Some("z_ai"))
        .await
        .unwrap();

    let retrieved = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(retrieved.upstream_provider.as_deref(), Some("z_ai"));
    assert_eq!(retrieved.provider_profile.as_deref(), Some("z_ai"));
}

#[tokio::test]
async fn test_update_coordination_mode() {
    let repo = MemoryChatConversationRepository::new();
    let conv = ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    let id = conv.id;

    repo.create(conv).await.unwrap();
    repo.update_coordination_mode(&id, CoordinationMode::RxNativeTeam)
        .await
        .unwrap();

    let retrieved = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(retrieved.coordination_mode, CoordinationMode::RxNativeTeam);
}

#[tokio::test]
async fn test_get_attribution_backfill_summary_counts_legacy_states() {
    let repo = MemoryChatConversationRepository::new();

    let session_a = IdeationSessionId::new();
    let session_b = IdeationSessionId::new();
    let session_c = IdeationSessionId::new();

    let mut pending = ChatConversation::new_ideation(session_a);
    pending.claude_session_id = Some("claude-pending".to_string());

    let mut running = ChatConversation::new_ideation(session_b);
    running.claude_session_id = Some("claude-running".to_string());
    running.attribution_backfill_status = Some(AttributionBackfillStatus::Running);

    let mut partial = ChatConversation::new_ideation(session_c);
    partial.claude_session_id = Some("claude-partial".to_string());
    partial.attribution_backfill_status = Some(AttributionBackfillStatus::Partial);

    repo.create(pending).await.unwrap();
    repo.create(running).await.unwrap();
    repo.create(partial).await.unwrap();
    repo.create(ChatConversation::new_project(
        crate::domain::entities::ProjectId::from_string("project-1".to_string()),
    ))
    .await
    .unwrap();

    let summary = repo.get_attribution_backfill_summary().await.unwrap();

    assert_eq!(summary.eligible_conversation_count, 3);
    assert_eq!(summary.pending_count, 1);
    assert_eq!(summary.running_count, 1);
    assert_eq!(summary.partial_count, 1);
    assert_eq!(summary.completed_count, 0);
    assert_eq!(summary.remaining_count(), 2);
    assert_eq!(summary.attention_count(), 1);
}

#[tokio::test]
async fn test_list_needing_attribution_backfill_only_returns_pending_work() {
    let repo = MemoryChatConversationRepository::new();

    let mut pending = ChatConversation::new_ideation(IdeationSessionId::new());
    pending.claude_session_id = Some("claude-pending".to_string());

    let mut running = ChatConversation::new_ideation(IdeationSessionId::new());
    running.claude_session_id = Some("claude-running".to_string());
    running.attribution_backfill_status = Some(AttributionBackfillStatus::Running);

    let mut partial = ChatConversation::new_ideation(IdeationSessionId::new());
    partial.claude_session_id = Some("claude-partial".to_string());
    partial.attribution_backfill_status = Some(AttributionBackfillStatus::Partial);

    let mut not_found = ChatConversation::new_ideation(IdeationSessionId::new());
    not_found.claude_session_id = Some("claude-not-found".to_string());
    not_found.attribution_backfill_status = Some(AttributionBackfillStatus::SessionNotFound);

    repo.create(pending.clone()).await.unwrap();
    repo.create(running).await.unwrap();
    repo.create(partial).await.unwrap();
    repo.create(not_found).await.unwrap();

    let needing = repo.list_needing_attribution_backfill(10).await.unwrap();

    assert_eq!(needing.len(), 1);
    assert_eq!(needing[0].id, pending.id);
}

#[tokio::test]
async fn test_reset_running_attribution_backfill_to_pending() {
    let repo = MemoryChatConversationRepository::new();

    let mut running = ChatConversation::new_ideation(IdeationSessionId::new());
    running.claude_session_id = Some("claude-running".to_string());
    running.attribution_backfill_status = Some(AttributionBackfillStatus::Running);
    let running_id = running.id;

    let mut completed = ChatConversation::new_ideation(IdeationSessionId::new());
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

#[tokio::test]
async fn test_list_by_automation_id_returns_only_matching_conversations() {
    use crate::domain::entities::{AutomationId, AutomationRunId};

    let repo = MemoryChatConversationRepository::new();
    let project_id = ProjectId::new();
    let automation_id = AutomationId::from_string("automation-mem-1");

    let mut setup = ChatConversation::new_project(project_id.clone());
    setup.automation_id = Some(automation_id.clone());
    let setup_id = setup.id;
    repo.create(setup).await.unwrap();

    let mut run_conv = ChatConversation::new_project(project_id.clone());
    run_conv.automation_id = Some(automation_id.clone());
    run_conv.automation_run_id = Some(AutomationRunId::from_string("run-1"));
    let run_id = run_conv.id;
    repo.create(run_conv).await.unwrap();

    // Archived conversation for the same automation is still returned.
    let mut archived = ChatConversation::new_project(project_id.clone());
    archived.automation_id = Some(automation_id.clone());
    let archived_id = archived.id;
    repo.create(archived).await.unwrap();
    repo.archive(&archived_id).await.unwrap();

    // Unrelated automation is excluded.
    let mut other = ChatConversation::new_project(project_id);
    other.automation_id = Some(AutomationId::from_string("automation-other"));
    repo.create(other).await.unwrap();

    let listed = repo.list_by_automation_id(&automation_id).await.unwrap();
    let ids: Vec<_> = listed.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&setup_id));
    assert!(ids.contains(&run_id));
    assert!(ids.contains(&archived_id));
}
