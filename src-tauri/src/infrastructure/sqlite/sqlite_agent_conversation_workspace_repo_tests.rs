use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    AgentWorkspacePrCommentEvidenceUpsert, AgentWorkspacePrDescription, ChatConversationId,
    IdeationAnalysisBaseRefKind, PlanBranchId, ProjectId,
    DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::testing::SqliteTestDb;

use super::SqliteAgentConversationWorkspaceRepository;

fn setup_repo() -> (
    SqliteTestDb,
    SqliteAgentConversationWorkspaceRepository,
    ChatConversationId,
) {
    let db = SqliteTestDb::new("sqlite_agent_conversation_workspace_repo_tests");
    let conversation_id = ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
    seed_conversation(&db, &conversation_id);
    let repo = SqliteAgentConversationWorkspaceRepository::from_shared(db.shared_conn());
    (db, repo, conversation_id)
}

fn seed_conversation(db: &SqliteTestDb, conversation_id: &ChatConversationId) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (
                id, context_type, context_id, title, message_count, created_at, updated_at
             ) VALUES (
                ?1, 'project', 'project-1', 'Workspace chat', 0,
                '2026-04-26T09:00:00Z', '2026-04-26T09:00:00Z'
             )",
            rusqlite::params![conversation_id.as_str()],
        )
        .unwrap();
    });
}

fn make_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/agent-11111111".to_string(),
        "/tmp/ralphx/agent-11111111".to_string(),
    )
}

#[tokio::test]
async fn terminal_cleanup_candidates_skip_marked_rows() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id);
    workspace.publication_pr_number = Some(72);
    workspace.publication_pr_status = Some("merged".to_string());

    repo.create_or_update(workspace).await.unwrap();
    assert_eq!(
        repo.get_terminal_local_cleanup_candidates_by_project_id(&ProjectId::from_string(
            "project-1".to_string()
        ))
        .await
        .unwrap()
        .len(),
        1
    );

    repo.mark_local_cleanup_status(&conversation_id, "cleaned", chrono::Utc::now())
        .await
        .unwrap();

    assert!(repo
        .get_terminal_local_cleanup_candidates_by_project_id(&ProjectId::from_string(
            "project-1".to_string()
        ))
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn terminal_cleanup_candidates_retry_unsafe_after_ttl() {
    let (db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id);
    workspace.publication_pr_number = Some(80);
    workspace.publication_pr_status = Some("closed".to_string());
    repo.create_or_update(workspace).await.unwrap();

    let old_timestamp = chrono::Utc::now() - chrono::Duration::hours(25);
    repo.mark_local_cleanup_status(&conversation_id, "unsafe", old_timestamp)
        .await
        .unwrap();

    let candidates = repo
        .get_terminal_local_cleanup_candidates_by_project_id(&ProjectId::from_string(
            "project-1".to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        candidates.len(),
        1,
        "unsafe with expired TTL should be retryable"
    );

    let recent_timestamp = chrono::Utc::now();
    repo.mark_local_cleanup_status(&conversation_id, "unsafe", recent_timestamp)
        .await
        .unwrap();

    let candidates = repo
        .get_terminal_local_cleanup_candidates_by_project_id(&ProjectId::from_string(
            "project-1".to_string(),
        ))
        .await
        .unwrap();
    assert!(
        candidates.is_empty(),
        "unsafe with fresh TTL should not be retryable"
    );

    let _ = db;
}

#[tokio::test]
async fn terminal_cleanup_candidates_retry_target_ref_missing_after_ttl() {
    let (db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id);
    workspace.publication_pr_number = Some(81);
    workspace.publication_pr_status = Some("merged".to_string());
    repo.create_or_update(workspace).await.unwrap();

    let old_timestamp = chrono::Utc::now() - chrono::Duration::hours(25);
    repo.mark_local_cleanup_status(&conversation_id, "target_ref_missing", old_timestamp)
        .await
        .unwrap();

    let candidates = repo
        .get_terminal_local_cleanup_candidates_by_project_id(&ProjectId::from_string(
            "project-1".to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        candidates.len(),
        1,
        "target_ref_missing with expired TTL should be retryable"
    );

    let _ = db;
}

#[tokio::test]
async fn list_worktree_paths_by_project_id_returns_paths() {
    let (db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id))
        .await
        .unwrap();

    let second_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &second_id);
    let mut second = make_workspace(second_id);
    second.worktree_path = "/tmp/ralphx/agent-22222222".to_string();
    repo.create_or_update(second).await.unwrap();

    let paths = repo
        .list_worktree_paths_by_project_id(&ProjectId::from_string("project-1".to_string()))
        .await
        .unwrap();

    assert_eq!(paths.len(), 2);
    assert!(paths.contains("/tmp/ralphx/agent-11111111"));
    assert!(paths.contains("/tmp/ralphx/agent-22222222"));
}

#[tokio::test]
async fn list_worktree_paths_by_project_id_empty_for_unknown_project() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id))
        .await
        .unwrap();

    let paths = repo
        .list_worktree_paths_by_project_id(&ProjectId::from_string("no-such-project".to_string()))
        .await
        .unwrap();
    assert!(paths.is_empty());
}

#[tokio::test]
async fn pr_description_round_trips_and_clears() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id))
        .await
        .unwrap();

    repo.save_pr_description(
        &conversation_id,
        AgentWorkspacePrDescription::new(
            Some("Describe agent workspace publish".to_string()),
            "## Summary\n\n- Added publish descriptions".to_string(),
        ),
    )
    .await
    .unwrap();

    let saved = repo
        .get_pr_description(&conversation_id)
        .await
        .unwrap()
        .expect("description should be saved");
    assert_eq!(
        saved.title.as_deref(),
        Some("Describe agent workspace publish")
    );
    assert!(saved.body_markdown.contains("## Summary"));

    repo.clear_pr_description(&conversation_id).await.unwrap();
    assert!(repo
        .get_pr_description(&conversation_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn publication_events_round_trip_in_created_order() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id))
        .await
        .unwrap();

    repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
        conversation_id,
        "checking",
        "started",
        "Checking workspace",
        None,
    ))
    .await
    .unwrap();
    repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
        conversation_id,
        "needs_agent",
        "failed",
        "Pre-commit hook failed",
        Some("agent_fixable".to_string()),
    ))
    .await
    .unwrap();

    let events = repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].step, "checking");
    assert_eq!(events[0].summary, "Checking workspace");
    assert_eq!(events[1].classification.as_deref(), Some("agent_fixable"));
}

#[tokio::test]
async fn pr_comment_evidence_tracks_edits_inclusion_and_reads() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();

    repo.upsert_pr_comment_evidence(
        &conversation_id,
        vec![AgentWorkspacePrCommentEvidenceUpsert::new(
            267,
            "comment-1".to_string(),
            Some("codecov".to_string()),
            "Patch coverage is below target.".to_string(),
            Some("https://github.com/owner/repo/pull/267#issuecomment-1".to_string()),
            Some("2026-05-18T22:00:00Z".to_string()),
            Some("2026-05-18T22:00:00Z".to_string()),
            true,
            true,
        )],
    )
    .await
    .unwrap();

    let first = repo
        .list_pr_comment_evidence(&conversation_id, 267, 10)
        .await
        .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].comment_id, "comment-1");
    assert_eq!(first[0].edit_count, 0);
    assert!(first[0].body_excerpt.contains("Patch coverage"));

    repo.mark_pr_comments_included(&conversation_id, 267, &["comment-1".to_string()])
        .await
        .unwrap();
    repo.mark_pr_comment_read(&conversation_id, 267, "comment-1")
        .await
        .unwrap();
    repo.upsert_pr_comment_evidence(
        &conversation_id,
        vec![AgentWorkspacePrCommentEvidenceUpsert::new(
            267,
            "comment-1".to_string(),
            Some("codecov".to_string()),
            "Patch coverage recovered after rerun.".to_string(),
            Some("https://github.com/owner/repo/pull/267#issuecomment-1".to_string()),
            Some("2026-05-18T22:00:00Z".to_string()),
            Some("2026-05-18T22:05:00Z".to_string()),
            true,
            true,
        )],
    )
    .await
    .unwrap();

    let updated = repo
        .get_pr_comment_evidence(&conversation_id, 267, "comment-1")
        .await
        .unwrap()
        .expect("comment should exist");
    assert_eq!(updated.edit_count, 1);
    assert_eq!(updated.body, "Patch coverage recovered after rerun.");
    assert!(updated.last_included_at.is_some());
    assert!(updated.last_read_at.is_some());
}

#[tokio::test]
async fn delete_removes_pr_comment_evidence_for_conversation() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();
    repo.upsert_pr_comment_evidence(
        &conversation_id,
        vec![AgentWorkspacePrCommentEvidenceUpsert::new(
            267,
            "comment-1".to_string(),
            Some("codecov".to_string()),
            "Patch coverage is below target.".to_string(),
            Some("https://github.com/owner/repo/pull/267#issuecomment-1".to_string()),
            Some("2026-05-18T22:00:00Z".to_string()),
            Some("2026-05-18T22:00:00Z".to_string()),
            true,
            true,
        )],
    )
    .await
    .unwrap();

    repo.delete(&conversation_id).await.unwrap();

    let comments = repo
        .list_pr_comment_evidence(&conversation_id, 267, 10)
        .await
        .unwrap();
    assert!(comments.is_empty());
}

#[tokio::test]
async fn list_active_direct_published_workspaces_filters_to_open_edit_workspaces() {
    let (db, repo, conversation_id) = setup_repo();
    let mut published = make_workspace(conversation_id);
    published.publication_pr_number = Some(72);
    published.publication_pr_url = Some("https://github.com/owner/repo/pull/72".to_string());
    published.publication_pr_status = Some("open".to_string());
    repo.create_or_update(published.clone()).await.unwrap();

    let refreshed_id = ChatConversationId::from_string("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
    seed_conversation(&db, &refreshed_id);
    let mut refreshed = make_workspace(refreshed_id);
    refreshed.publication_pr_number = Some(78);
    refreshed.publication_pr_status = Some("open".to_string());
    refreshed.publication_push_status = Some("refreshed".to_string());
    repo.create_or_update(refreshed.clone()).await.unwrap();

    let archived_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &archived_id);
    let mut archived = make_workspace(archived_id);
    archived.status = AgentConversationWorkspaceStatus::Archived;
    archived.publication_pr_number = Some(73);
    repo.create_or_update(archived).await.unwrap();

    let execution_owned_id =
        ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
    seed_conversation(&db, &execution_owned_id);
    let mut execution_owned = make_workspace(execution_owned_id);
    execution_owned.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-branch-1"));
    execution_owned.publication_pr_number = Some(74);
    repo.create_or_update(execution_owned).await.unwrap();

    let ideation_id = ChatConversationId::from_string("77777777-7777-7777-7777-777777777777");
    seed_conversation(&db, &ideation_id);
    let mut ideation = make_workspace(ideation_id);
    ideation.mode = AgentConversationWorkspaceMode::Ideation;
    ideation.publication_pr_number = Some(77);
    ideation.publication_pr_status = Some("open".to_string());
    ideation.publication_push_status = Some("pushed".to_string());
    repo.create_or_update(ideation).await.unwrap();

    let closed_id = ChatConversationId::from_string("44444444-4444-4444-4444-444444444444");
    seed_conversation(&db, &closed_id);
    let mut closed = make_workspace(closed_id);
    closed.publication_pr_number = Some(75);
    closed.publication_pr_status = Some("closed".to_string());
    repo.create_or_update(closed).await.unwrap();

    let needs_agent_id = ChatConversationId::from_string("55555555-5555-5555-5555-555555555555");
    seed_conversation(&db, &needs_agent_id);
    let mut needs_agent = make_workspace(needs_agent_id);
    needs_agent.publication_pr_number = Some(76);
    needs_agent.publication_pr_status = Some("changes_requested".to_string());
    needs_agent.publication_push_status = Some("needs_agent".to_string());
    repo.create_or_update(needs_agent).await.unwrap();

    let workspaces = repo
        .list_active_direct_published_workspaces()
        .await
        .unwrap();

    assert_eq!(workspaces.len(), 2);
    assert!(workspaces
        .iter()
        .any(|workspace| workspace.conversation_id == published.conversation_id));
    assert!(workspaces
        .iter()
        .any(|workspace| workspace.conversation_id == refreshed.conversation_id));
}

#[tokio::test]
async fn list_external_pr_reconciliation_candidates_filters_to_unlinked_active_edit_workspaces() {
    let (db, repo, conversation_id) = setup_repo();
    let candidate = make_workspace(conversation_id);
    repo.create_or_update(candidate.clone()).await.unwrap();

    let linked_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &linked_id);
    let mut linked = make_workspace(linked_id);
    linked.publication_pr_number = Some(72);
    linked.publication_pr_status = Some("open".to_string());
    repo.create_or_update(linked).await.unwrap();

    let needs_agent_id = ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
    seed_conversation(&db, &needs_agent_id);
    let mut needs_agent = make_workspace(needs_agent_id);
    needs_agent.publication_push_status = Some("needs_agent".to_string());
    repo.create_or_update(needs_agent).await.unwrap();

    let ideation_id = ChatConversationId::from_string("44444444-4444-4444-4444-444444444444");
    seed_conversation(&db, &ideation_id);
    let mut ideation = make_workspace(ideation_id);
    ideation.mode = AgentConversationWorkspaceMode::Ideation;
    repo.create_or_update(ideation).await.unwrap();

    let workspaces = repo
        .list_active_direct_external_pr_reconciliation_candidates(10)
        .await
        .unwrap();

    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].conversation_id, candidate.conversation_id);

    let limited = repo
        .list_active_direct_external_pr_reconciliation_candidates(0)
        .await
        .unwrap();
    assert!(limited.is_empty());
}

#[tokio::test]
async fn list_active_direct_pr_supervision_recovery_candidates_filters_blocked_failed_prs() {
    let (db, repo, conversation_id) = setup_repo();
    let mut candidate = make_workspace(conversation_id);
    candidate.publication_pr_number = Some(82);
    candidate.publication_pr_status = Some("open".to_string());
    candidate.publication_push_status = Some("failed".to_string());
    candidate.pr_supervision_status = Some("blocked".to_string());
    candidate.pr_autofix_enabled = true;
    repo.create_or_update(candidate.clone()).await.unwrap();

    let disabled_id = ChatConversationId::from_string("66666666-6666-6666-6666-666666666666");
    seed_conversation(&db, &disabled_id);
    let mut disabled = make_workspace(disabled_id);
    disabled.publication_pr_number = Some(83);
    disabled.publication_pr_status = Some("open".to_string());
    disabled.publication_push_status = Some("failed".to_string());
    disabled.pr_supervision_status = Some("blocked".to_string());
    repo.create_or_update(disabled).await.unwrap();

    let needs_agent_id = ChatConversationId::from_string("77777777-7777-7777-7777-777777777777");
    seed_conversation(&db, &needs_agent_id);
    let mut needs_agent = make_workspace(needs_agent_id);
    needs_agent.publication_pr_number = Some(84);
    needs_agent.publication_pr_status = Some("open".to_string());
    needs_agent.publication_push_status = Some("needs_agent".to_string());
    needs_agent.pr_supervision_status = Some("blocked".to_string());
    needs_agent.pr_autofix_enabled = true;
    repo.create_or_update(needs_agent).await.unwrap();

    let closed_id = ChatConversationId::from_string("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
    seed_conversation(&db, &closed_id);
    let mut closed = make_workspace(closed_id);
    closed.publication_pr_number = Some(85);
    closed.publication_pr_status = Some("closed".to_string());
    closed.publication_push_status = Some("failed".to_string());
    closed.pr_supervision_status = Some("blocked".to_string());
    closed.pr_autofix_enabled = true;
    repo.create_or_update(closed).await.unwrap();

    let workspaces = repo
        .list_active_direct_pr_supervision_recovery_candidates(10)
        .await
        .unwrap();

    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].conversation_id, candidate.conversation_id);

    let limited = repo
        .list_active_direct_pr_supervision_recovery_candidates(0)
        .await
        .unwrap();
    assert!(limited.is_empty());
}

#[tokio::test]
async fn list_active_needs_agent_workspaces_filters_to_open_active_workspaces() {
    let (db, repo, conversation_id) = setup_repo();
    let mut needs_agent = make_workspace(conversation_id);
    needs_agent.publication_pr_number = Some(82);
    needs_agent.publication_pr_status = Some("failed".to_string());
    needs_agent.publication_push_status = Some("needs_agent".to_string());
    repo.create_or_update(needs_agent.clone()).await.unwrap();

    let closed_id = ChatConversationId::from_string("88888888-8888-8888-8888-888888888888");
    seed_conversation(&db, &closed_id);
    let mut closed = make_workspace(closed_id);
    closed.publication_pr_number = Some(83);
    closed.publication_pr_status = Some("closed".to_string());
    closed.publication_push_status = Some("needs_agent".to_string());
    repo.create_or_update(closed).await.unwrap();

    let archived_id = ChatConversationId::from_string("99999999-9999-9999-9999-999999999999");
    seed_conversation(&db, &archived_id);
    let mut archived = make_workspace(archived_id);
    archived.status = AgentConversationWorkspaceStatus::Archived;
    archived.publication_pr_number = Some(84);
    archived.publication_pr_status = Some("failed".to_string());
    archived.publication_push_status = Some("needs_agent".to_string());
    repo.create_or_update(archived).await.unwrap();

    let pushed_id = ChatConversationId::from_string("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
    seed_conversation(&db, &pushed_id);
    let mut pushed = make_workspace(pushed_id);
    pushed.publication_pr_number = Some(85);
    pushed.publication_pr_status = Some("open".to_string());
    pushed.publication_push_status = Some("pushed".to_string());
    repo.create_or_update(pushed).await.unwrap();

    let workspaces = repo.list_active_needs_agent_workspaces().await.unwrap();

    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].conversation_id, needs_agent.conversation_id);
    assert_eq!(
        workspaces[0].publication_push_status.as_deref(),
        Some("needs_agent")
    );
}

#[tokio::test]
async fn pr_supervision_preferences_round_trip() {
    let (_db, repo, conversation_id) = setup_repo();
    let workspace = make_workspace(conversation_id.clone());
    repo.create_or_update(workspace).await.unwrap();

    repo.update_pr_supervision_preferences(&conversation_id, true, true, "squash")
        .await
        .unwrap();

    let updated = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert!(updated.pr_autofix_enabled);
    assert!(updated.pr_auto_merge_desired);
    assert_eq!(
        updated.pr_auto_merge_method,
        DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD
    );
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    assert!(updated.pr_supervision_updated_at.is_some());
}

#[tokio::test]
async fn terminal_publication_update_clears_stale_pr_supervision_state() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.publication_pr_number = Some(91);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("failed".to_string());
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_supervision_summary = Some("CI checks failed".to_string());
    workspace.pr_supervision_updated_at = Some(chrono::Utc::now());
    repo.create_or_update(workspace).await.unwrap();

    repo.update_publication(
        &conversation_id,
        Some(91),
        Some("https://github.com/owner/repo/pull/91"),
        Some("merged"),
        Some("pushed"),
    )
    .await
    .unwrap();

    let updated = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_status.as_deref(), Some("merged"));
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert!(updated.pr_supervision_status.is_none());
    assert!(updated.pr_supervision_summary.is_none());
}
