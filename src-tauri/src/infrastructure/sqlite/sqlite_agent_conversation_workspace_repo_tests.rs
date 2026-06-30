use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentConversationWorkspacePublicationEvent,
    AgentConversationWorkspaceStatus, AgentWorkspaceFollowupProvenance,
    AgentWorkspacePrCommentEvidenceUpsert, AgentWorkspacePrDescription,
    AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, AgentWorkspaceSourcePullRequest, ArtifactId,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranchId, ProjectId,
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

fn set_workspace_updated_at(
    db: &SqliteTestDb,
    conversation_id: &ChatConversationId,
    updated_at: chrono::DateTime<chrono::Utc>,
) {
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE agent_conversation_workspaces
             SET updated_at = ?2
             WHERE conversation_id = ?1",
            rusqlite::params![conversation_id.as_str(), updated_at.to_rfc3339()],
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
async fn source_pull_request_metadata_round_trips() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id);
    workspace.base_ref_kind = IdeationAnalysisBaseRefKind::LocalBranch;
    workspace.base_ref = "feature/pr-origin".to_string();
    workspace.base_display_name = Some("PR #123: Add PR context".to_string());
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 123,
        url: Some("https://github.com/owner/repo/pull/123".to_string()),
        title: Some("Add PR context".to_string()),
        head_ref_name: "feature/pr-origin".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("abc123".to_string()),
    });

    repo.create_or_update(workspace).await.unwrap();

    let loaded = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should load");
    assert_eq!(
        loaded.source_pull_request,
        Some(AgentWorkspaceSourcePullRequest {
            number: 123,
            url: Some("https://github.com/owner/repo/pull/123".to_string()),
            title: Some("Add PR context".to_string()),
            head_ref_name: "feature/pr-origin".to_string(),
            base_ref_name: Some("main".to_string()),
            head_ref_oid: Some("abc123".to_string()),
        })
    );
}

#[tokio::test]
async fn branch_mode_round_trips_and_defaults_to_isolated() {
    let (db, repo, conversation_id) = setup_repo();
    let workspace = make_workspace(conversation_id.clone());
    repo.create_or_update(workspace).await.unwrap();
    let loaded = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should load");
    assert_eq!(
        loaded.branch_mode,
        AgentConversationWorkspaceBranchMode::Isolated
    );

    let second_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &second_id);
    let mut linked = make_workspace(second_id.clone());
    linked.branch_mode = AgentConversationWorkspaceBranchMode::Linked;
    linked.branch_name = "feature/existing-pr".to_string();
    linked.worktree_path = "/tmp/ralphx/existing-pr".to_string();
    repo.create_or_update(linked).await.unwrap();

    let loaded = repo
        .get_by_conversation_id(&second_id)
        .await
        .unwrap()
        .expect("linked workspace should load");
    assert_eq!(
        loaded.branch_mode,
        AgentConversationWorkspaceBranchMode::Linked
    );
}

#[tokio::test]
async fn active_branch_lookup_ignores_terminal_workspace_statuses() {
    let (db, repo, first_id) = setup_repo();
    let project_id = ProjectId::from_string("project-1".to_string());
    let mut first = make_workspace(first_id);
    first.branch_name = "feature/shared".to_string();
    repo.create_or_update(first).await.unwrap();

    let archived_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &archived_id);
    let mut archived = make_workspace(archived_id.clone());
    archived.branch_name = "feature/shared".to_string();
    archived.worktree_path = "/tmp/ralphx/archived".to_string();
    archived.status = AgentConversationWorkspaceStatus::Archived;
    repo.create_or_update(archived).await.unwrap();

    let found = repo
        .find_active_by_project_and_branch_name(&project_id, "feature/shared")
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].branch_name, "feature/shared");
    assert_eq!(found[0].status, AgentConversationWorkspaceStatus::Active);

    let missing = repo
        .find_active_by_project_and_branch_name(&project_id, "   ")
        .await
        .unwrap();
    assert!(missing.is_empty());
}

#[tokio::test]
async fn find_by_head_ref_matches_only_same_project_branch() {
    let (db, repo, first_id) = setup_repo();
    let mut first = make_workspace(first_id.clone());
    first.branch_name = "shared/feature-branch".to_string();
    repo.create_or_update(first).await.unwrap();

    // A different project's workspace shares the same branch name — it must NOT
    // be returned (branch_name is global; the project_id predicate is mandatory).
    let second_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &second_id);
    let mut second = make_workspace(second_id.clone());
    second.project_id = ProjectId::from_string("project-2".to_string());
    second.branch_name = "shared/feature-branch".to_string();
    second.worktree_path = "/tmp/ralphx/agent-22222222".to_string();
    repo.create_or_update(second).await.unwrap();

    let project_1 = ProjectId::from_string("project-1".to_string());
    let matches = repo
        .find_by_head_ref(&project_1, "shared/feature-branch")
        .await
        .unwrap();

    assert_eq!(
        matches.len(),
        1,
        "only the project-1 workspace should match"
    );
    assert_eq!(matches[0].conversation_id, first_id);
    assert_eq!(matches[0].project_id, project_1);
}

#[tokio::test]
async fn find_by_head_ref_returns_empty_when_no_branch_match() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id);
    workspace.branch_name = "ralphx/project/real-branch".to_string();
    repo.create_or_update(workspace).await.unwrap();

    let project_1 = ProjectId::from_string("project-1".to_string());
    let matches = repo
        .find_by_head_ref(&project_1, "does/not/exist")
        .await
        .unwrap();

    assert!(
        matches.is_empty(),
        "no branch match yields an empty vec, not an error"
    );
}

#[tokio::test]
async fn linked_ideation_session_lookup_returns_latest_workspace_and_none_for_missing() {
    let (db, repo, first_id) = setup_repo();
    let session_id = IdeationSessionId::from_string("ideation-session-1");
    let mut first = make_workspace(first_id);
    first.linked_ideation_session_id = Some(session_id.clone());
    first.branch_name = "ralphx/project/agent-first".to_string();
    repo.create_or_update(first).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    let second_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &second_id);
    let mut second = make_workspace(second_id.clone());
    second.linked_ideation_session_id = Some(session_id.clone());
    second.branch_name = "ralphx/project/agent-second".to_string();
    second.worktree_path = "/tmp/ralphx/agent-22222222".to_string();
    repo.create_or_update(second).await.unwrap();

    let loaded = repo
        .get_by_linked_ideation_session_id(&session_id)
        .await
        .unwrap()
        .expect("latest linked workspace should load");
    assert_eq!(loaded.conversation_id, second_id);
    assert_eq!(loaded.branch_name, "ralphx/project/agent-second");

    let missing = repo
        .get_by_linked_ideation_session_id(&IdeationSessionId::from_string("missing-session"))
        .await
        .unwrap();
    assert!(missing.is_none());
}

#[tokio::test]
async fn followup_blocker_lookup_returns_latest_active_workspace() {
    let (db, repo, first_id) = setup_repo();
    let origin_id = ChatConversationId::from_string("origin-conversation");

    let mut first = make_workspace(first_id.clone());
    first.mode = AgentConversationWorkspaceMode::Ideation;
    repo.create_or_update(first).await.unwrap();
    repo.save_followup_provenance(
        &first_id,
        AgentWorkspaceFollowupProvenance {
            origin_conversation_id: origin_id.clone(),
            source_task_id: Some("task-1".to_string()),
            source_context_type: Some("task".to_string()),
            source_context_id: Some("task-1".to_string()),
            source_agent_name: Some("ralphx-execution-worker".to_string()),
            spawn_reason: Some("out_of_scope_failure".to_string()),
            blocker_fingerprint: Some("scope-drift:task-1:file".to_string()),
        },
    )
    .await
    .unwrap();
    repo.update_status(&first_id, AgentConversationWorkspaceStatus::Archived)
        .await
        .unwrap();

    let second_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &second_id);
    let mut second = make_workspace(second_id.clone());
    second.mode = AgentConversationWorkspaceMode::Ideation;
    second.branch_name = "ralphx/project/agent-second".to_string();
    second.worktree_path = "/tmp/ralphx/agent-22222222".to_string();
    repo.create_or_update(second).await.unwrap();
    repo.save_followup_provenance(
        &second_id,
        AgentWorkspaceFollowupProvenance {
            origin_conversation_id: origin_id.clone(),
            source_task_id: Some("task-1".to_string()),
            source_context_type: Some("task".to_string()),
            source_context_id: Some("task-1".to_string()),
            source_agent_name: Some("ralphx-execution-reviewer".to_string()),
            spawn_reason: Some("out_of_scope_failure".to_string()),
            blocker_fingerprint: Some("scope-drift:task-1:file".to_string()),
        },
    )
    .await
    .unwrap();

    let found = repo
        .find_active_followup_by_blocker(&origin_id, "task-1", "scope-drift:task-1:file")
        .await
        .unwrap()
        .expect("active matching follow-up should be found");
    assert_eq!(found.conversation_id, second_id);

    let missing = repo
        .find_active_followup_by_blocker(&origin_id, "task-1", "scope-drift:task-1:other")
        .await
        .unwrap();
    assert!(missing.is_none());
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
async fn workspace_review_monitor_round_trips_and_preserves_versioned_artifacts() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();

    let artifact_updated_at = chrono::Utc::now();
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-current"));
    monitor.review_artifact_version = Some(4);
    monitor.review_artifact_updated_at = Some(artifact_updated_at);
    monitor.review_conversation_id = Some(ChatConversationId::from_string(
        "22222222-2222-2222-2222-222222222222",
    ));
    monitor.reviewed_head_sha = Some("head-sha".to_string());
    monitor.reviewed_diff_fingerprint = Some("fingerprint".to_string());
    monitor.selected_source_base_ref = Some("main".to_string());
    monitor.selected_source_base_sha = Some("base-sha".to_string());
    monitor.selected_source_head_ref = Some("feature/review".to_string());
    monitor.selected_source_head_sha = Some("head-sha".to_string());
    monitor.selected_source_pull_request_number = Some(483);
    monitor.current_diff_fingerprint = Some("fingerprint".to_string());
    monitor.previous_version_id = Some(ArtifactId::from_string("artifact-previous"));
    monitor.review_blocking_summary = Some("Fix the stale review state.".to_string());
    monitor.review_blocking_fingerprint = Some("blocking-fingerprint".to_string());
    monitor.review_fixer_run_id = Some("fixer-run-1".to_string());
    monitor.review_fixer_conversation_id = Some(ChatConversationId::from_string(
        "33333333-3333-3333-3333-333333333333",
    ));
    monitor.review_fixer_status = Some("running".to_string());
    monitor.last_run_id = Some("run-1".to_string());

    let saved = repo.upsert_workspace_review_monitor(monitor).await.unwrap();
    assert_eq!(saved.status, AgentWorkspaceReviewMonitorStatus::Ready);
    assert_eq!(saved.review_outcome, AgentWorkspaceReviewOutcome::Blocking);
    assert_eq!(
        saved.review_gate_status,
        AgentWorkspaceReviewGateStatus::Blocking
    );
    assert_eq!(
        saved.current_target_scope,
        Some(AgentWorkspaceReviewTargetScope::SelectedSource)
    );
    assert_eq!(saved.review_artifact_version, Some(4));
    assert_eq!(
        saved.review_artifact_id.as_ref().map(ArtifactId::as_str),
        Some("artifact-current")
    );
    assert_eq!(
        saved
            .review_conversation_id
            .as_ref()
            .map(ChatConversationId::as_str),
        Some("22222222-2222-2222-2222-222222222222".to_string())
    );
    assert_eq!(
        saved.previous_version_id.as_ref().map(ArtifactId::as_str),
        Some("artifact-previous")
    );
    assert_eq!(saved.selected_source_pull_request_number, Some(483));
    assert_eq!(
        saved.review_blocking_summary.as_deref(),
        Some("Fix the stale review state.")
    );
    assert_eq!(
        saved.review_blocking_fingerprint.as_deref(),
        Some("blocking-fingerprint")
    );
    assert_eq!(saved.review_fixer_run_id.as_deref(), Some("fixer-run-1"));
    assert_eq!(
        saved
            .review_fixer_conversation_id
            .as_ref()
            .map(ChatConversationId::as_str),
        Some("33333333-3333-3333-3333-333333333333".to_string())
    );
    assert_eq!(saved.review_fixer_status.as_deref(), Some("running"));
    assert_eq!(saved.last_run_id.as_deref(), Some("run-1"));

    let mut update = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    update.status = AgentWorkspaceReviewMonitorStatus::Blocked;
    update.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
    update.review_gate_status = AgentWorkspaceReviewGateStatus::Failed;
    update.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    update.workspace_base_ref = Some("base-sha".to_string());
    update.workspace_head_ref = Some("HEAD".to_string());
    update.current_diff_fingerprint = Some("new-fingerprint".to_string());
    update.last_run_id = Some("run-2".to_string());
    update.last_error = Some("review failed".to_string());

    let updated = repo.upsert_workspace_review_monitor(update).await.unwrap();
    assert_eq!(updated.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        updated.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        updated.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert_eq!(
        updated.current_target_scope,
        Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta)
    );
    assert_eq!(updated.workspace_base_ref.as_deref(), Some("base-sha"));
    assert_eq!(updated.workspace_head_ref.as_deref(), Some("HEAD"));
    assert_eq!(
        updated.current_diff_fingerprint.as_deref(),
        Some("new-fingerprint")
    );
    assert_eq!(
        updated.review_artifact_id.as_ref().map(ArtifactId::as_str),
        Some("artifact-current"),
        "partial monitor updates should preserve the last artifact id"
    );
    assert_eq!(updated.review_artifact_version, Some(4));
    assert_eq!(
        updated
            .review_conversation_id
            .as_ref()
            .map(ChatConversationId::as_str),
        Some("22222222-2222-2222-2222-222222222222".to_string()),
        "partial monitor updates should preserve the active Review chat id"
    );
    assert_eq!(
        updated.previous_version_id.as_ref().map(ArtifactId::as_str),
        Some("artifact-previous")
    );
    assert_eq!(updated.last_run_id.as_deref(), Some("run-2"));
    assert_eq!(updated.last_error.as_deref(), Some("review failed"));
    assert_eq!(updated.review_blocking_summary, None);
    assert_eq!(updated.review_blocking_fingerprint, None);
    assert_eq!(updated.review_fixer_run_id, None);
    assert_eq!(updated.review_fixer_conversation_id, None);
    assert_eq!(updated.review_fixer_status, None);

    let loaded = repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .unwrap()
        .expect("monitor should load");
    assert_eq!(loaded, updated);
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
async fn pr_review_monitor_round_trips_and_active_listing_filters_terminal_rows() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();

    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        267,
        Some("head-sha-1".to_string()),
    );
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    monitor.monitor_enabled = true;
    monitor.first_review_completed = true;
    monitor.last_reviewed_head_sha = Some("head-sha-1".to_string());
    monitor.last_review_outcome = Some("request_changes".to_string());
    monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-v1"));
    monitor.review_artifact_head_sha = Some("head-sha-1".to_string());
    monitor.review_artifact_version = Some(1);
    monitor.review_artifact_updated_at = Some(chrono::Utc::now());

    let saved = repo
        .upsert_pr_review_monitor(monitor.clone())
        .await
        .unwrap();
    assert_eq!(saved.status, AgentWorkspacePrReviewMonitorStatus::Watching);
    assert_eq!(saved.last_seen_head_sha.as_deref(), Some("head-sha-1"));

    let loaded = repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .unwrap()
        .expect("monitor should exist");
    assert!(loaded.monitor_enabled);
    assert!(loaded.first_review_completed);
    assert_eq!(
        loaded.review_artifact_id.as_ref().map(|id| id.as_str()),
        Some("artifact-v1")
    );
    assert_eq!(
        loaded.review_artifact_head_sha.as_deref(),
        Some("head-sha-1")
    );
    assert_eq!(loaded.review_artifact_version, Some(1));
    assert!(loaded.review_artifact_updated_at.is_some());

    let active = repo.list_active_pr_review_monitors().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].conversation_id, conversation_id);

    let mut status_only_update = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        267,
        Some("head-sha-2".to_string()),
    );
    status_only_update.status = AgentWorkspacePrReviewMonitorStatus::Reviewing;
    let preserved = repo
        .upsert_pr_review_monitor(status_only_update)
        .await
        .unwrap();
    assert_eq!(
        preserved.review_artifact_id.as_ref().map(|id| id.as_str()),
        Some("artifact-v1")
    );
    assert_eq!(
        preserved.review_artifact_head_sha.as_deref(),
        Some("head-sha-1")
    );
    assert_eq!(preserved.review_artifact_version, Some(1));

    monitor.status = AgentWorkspacePrReviewMonitorStatus::Terminal;
    repo.upsert_pr_review_monitor(monitor).await.unwrap();
    assert!(repo
        .list_active_pr_review_monitors()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn pr_review_actions_update_existing_pending_action_for_same_head() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();

    let action = AgentWorkspacePrReviewAction::new(
        conversation_id.clone(),
        267,
        "head-sha-1".to_string(),
        AgentWorkspacePrReviewActionKind::RequestChanges,
        "Found blocking issues".to_string(),
        "Please address the blocking issues.".to_string(),
        Some(r#"[{"path":"src/lib.rs"}]"#.to_string()),
        Some("run-1".to_string()),
    );
    let saved = repo
        .create_or_update_pr_review_action(action)
        .await
        .unwrap();

    let replacement = AgentWorkspacePrReviewAction::new(
        conversation_id.clone(),
        267,
        "head-sha-1".to_string(),
        AgentWorkspacePrReviewActionKind::Approve,
        "Looks good now".to_string(),
        "The requested changes were addressed.".to_string(),
        None,
        Some("run-2".to_string()),
    );
    let updated = repo
        .create_or_update_pr_review_action(replacement)
        .await
        .unwrap();

    assert_eq!(updated.id, saved.id);
    assert_eq!(
        updated.proposed_action,
        AgentWorkspacePrReviewActionKind::Approve
    );
    assert_eq!(updated.summary, "Looks good now");
    assert_eq!(updated.created_by_run_id.as_deref(), Some("run-2"));

    let pending = repo
        .get_pending_pr_review_action_for_head(&conversation_id, 267, "head-sha-1")
        .await
        .unwrap()
        .expect("pending action should exist");
    assert_eq!(pending.id, saved.id);

    let actions = repo
        .list_pr_review_actions(&conversation_id, 10)
        .await
        .unwrap();
    assert_eq!(actions.len(), 1);

    repo.update_pr_review_action_status(
        &saved.id,
        AgentWorkspacePrReviewActionStatus::Submitted,
        Some("review-1"),
    )
    .await
    .unwrap();

    let submitted = repo
        .get_pr_review_action(&saved.id)
        .await
        .unwrap()
        .expect("action should still exist");
    assert_eq!(
        submitted.status,
        AgentWorkspacePrReviewActionStatus::Submitted
    );
    assert_eq!(submitted.submitted_review_id.as_deref(), Some("review-1"));
    assert!(submitted.resolved_at.is_some());
    assert!(repo
        .get_pending_pr_review_action_for_head(&conversation_id, 267, "head-sha-1")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn delete_removes_pr_review_state_for_conversation() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();
    let monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        267,
        Some("head-sha-1".to_string()),
    );
    repo.upsert_pr_review_monitor(monitor).await.unwrap();
    let action = AgentWorkspacePrReviewAction::new(
        conversation_id.clone(),
        267,
        "head-sha-1".to_string(),
        AgentWorkspacePrReviewActionKind::RequestChanges,
        "Found blocking issues".to_string(),
        "Please address the blocking issues.".to_string(),
        None,
        None,
    );
    repo.create_or_update_pr_review_action(action)
        .await
        .unwrap();

    repo.delete(&conversation_id).await.unwrap();

    assert!(repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .unwrap()
        .is_none());
    assert!(repo
        .list_pr_review_actions(&conversation_id, 10)
        .await
        .unwrap()
        .is_empty());
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

    let paused_id = ChatConversationId::from_string("12121212-1212-1212-1212-121212121212");
    seed_conversation(&db, &paused_id);
    let mut paused = make_workspace(paused_id);
    paused.publication_pr_number = Some(79);
    paused.publication_pr_status = Some("open".to_string());
    paused.publication_push_status = Some("pushed".to_string());
    paused.auto_publish_enabled = false;
    repo.create_or_update(paused).await.unwrap();

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
async fn list_active_pr_poller_recovery_workspaces_includes_supervised_ideation_prs() {
    let (db, repo, conversation_id) = setup_repo();
    let mut direct = make_workspace(conversation_id);
    direct.publication_pr_number = Some(72);
    direct.publication_pr_status = Some("open".to_string());
    direct.publication_push_status = Some("pushed".to_string());
    repo.create_or_update(direct.clone()).await.unwrap();

    let ideation_id = ChatConversationId::from_string("10101010-1010-1010-1010-101010101010");
    seed_conversation(&db, &ideation_id);
    let mut ideation = make_workspace(ideation_id);
    ideation.mode = AgentConversationWorkspaceMode::Ideation;
    ideation.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-branch-1"));
    ideation.publication_pr_number = Some(73);
    ideation.publication_pr_status = Some("open".to_string());
    ideation.publication_push_status = Some("pushed".to_string());
    ideation.pr_autofix_enabled = true;
    repo.create_or_update(ideation.clone()).await.unwrap();

    let unsupervised_id = ChatConversationId::from_string("20202020-2020-2020-2020-202020202020");
    seed_conversation(&db, &unsupervised_id);
    let mut unsupervised = make_workspace(unsupervised_id);
    unsupervised.mode = AgentConversationWorkspaceMode::Ideation;
    unsupervised.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-branch-2"));
    unsupervised.publication_pr_number = Some(74);
    unsupervised.publication_pr_status = Some("open".to_string());
    unsupervised.publication_push_status = Some("pushed".to_string());
    repo.create_or_update(unsupervised).await.unwrap();

    let workspaces = repo
        .list_active_pr_poller_recovery_workspaces()
        .await
        .unwrap();

    assert_eq!(
        workspaces
            .into_iter()
            .map(|workspace| workspace.conversation_id)
            .collect::<std::collections::HashSet<_>>(),
        [direct.conversation_id, ideation.conversation_id]
            .into_iter()
            .collect()
    );
}

#[tokio::test]
async fn list_external_pr_reconciliation_candidates_filters_to_reconcilable_edit_workspaces() {
    let (db, repo, conversation_id) = setup_repo();
    let candidate = make_workspace(conversation_id);
    repo.create_or_update(candidate.clone()).await.unwrap();

    let linked_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &linked_id);
    let mut linked = make_workspace(linked_id);
    linked.publication_pr_number = Some(72);
    linked.publication_pr_status = Some("open".to_string());
    linked.publication_push_status = Some("failed".to_string());
    repo.create_or_update(linked.clone()).await.unwrap();

    let missing_linked_id = ChatConversationId::from_string("25252525-2525-2525-2525-252525252525");
    seed_conversation(&db, &missing_linked_id);
    let mut missing_linked = make_workspace(missing_linked_id);
    missing_linked.status = AgentConversationWorkspaceStatus::Missing;
    missing_linked.publication_pr_number = Some(73);
    missing_linked.publication_pr_status = Some("open".to_string());
    missing_linked.publication_push_status = Some("needs_agent".to_string());
    repo.create_or_update(missing_linked.clone()).await.unwrap();

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

    assert_eq!(
        workspaces
            .into_iter()
            .map(|workspace| workspace.conversation_id)
            .collect::<std::collections::HashSet<_>>(),
        [
            candidate.conversation_id,
            linked.conversation_id,
            missing_linked.conversation_id
        ]
        .into_iter()
        .collect()
    );

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

    let paused_id = ChatConversationId::from_string("12121212-1212-1212-1212-121212121212");
    seed_conversation(&db, &paused_id);
    let mut paused = make_workspace(paused_id);
    paused.publication_pr_number = Some(86);
    paused.publication_pr_status = Some("open".to_string());
    paused.publication_push_status = Some("failed".to_string());
    paused.pr_supervision_status = Some("blocked".to_string());
    paused.pr_autofix_enabled = true;
    paused.auto_publish_enabled = false;
    repo.create_or_update(paused).await.unwrap();

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
async fn list_active_transient_publish_status_workspaces_filters_stale_open_rows() {
    let (db, repo, conversation_id) = setup_repo();
    let stale = chrono::Utc::now() - chrono::Duration::minutes(10);
    let older = chrono::Utc::now() - chrono::Duration::minutes(20);

    let mut refreshing = make_workspace(conversation_id);
    refreshing.publication_pr_number = Some(91);
    refreshing.publication_pr_status = Some("open".to_string());
    refreshing.publication_push_status = Some("refreshing".to_string());
    repo.create_or_update(refreshing.clone()).await.unwrap();
    set_workspace_updated_at(&db, &refreshing.conversation_id, stale);

    let describing_id = ChatConversationId::from_string("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
    seed_conversation(&db, &describing_id);
    let mut describing = make_workspace(describing_id);
    describing.publication_pr_number = Some(92);
    describing.publication_pr_status = Some("open".to_string());
    describing.publication_push_status = Some("describing".to_string());
    repo.create_or_update(describing.clone()).await.unwrap();
    set_workspace_updated_at(&db, &describing.conversation_id, older);

    let recent_id = ChatConversationId::from_string("cccccccc-cccc-cccc-cccc-cccccccccccc");
    seed_conversation(&db, &recent_id);
    let mut recent = make_workspace(recent_id);
    recent.publication_pr_number = Some(93);
    recent.publication_pr_status = Some("open".to_string());
    recent.publication_push_status = Some("checking".to_string());
    repo.create_or_update(recent).await.unwrap();

    let closed_id = ChatConversationId::from_string("dddddddd-dddd-dddd-dddd-dddddddddddd");
    seed_conversation(&db, &closed_id);
    let mut closed = make_workspace(closed_id);
    closed.publication_pr_number = Some(94);
    closed.publication_pr_status = Some("closed".to_string());
    closed.publication_push_status = Some("committing".to_string());
    repo.create_or_update(closed.clone()).await.unwrap();
    set_workspace_updated_at(&db, &closed.conversation_id, stale);

    let pushed_id = ChatConversationId::from_string("eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee");
    seed_conversation(&db, &pushed_id);
    let mut pushed = make_workspace(pushed_id);
    pushed.publication_pr_number = Some(95);
    pushed.publication_pr_status = Some("open".to_string());
    pushed.publication_push_status = Some("pushed".to_string());
    repo.create_or_update(pushed.clone()).await.unwrap();
    set_workspace_updated_at(&db, &pushed.conversation_id, stale);

    let archived_id = ChatConversationId::from_string("ffffffff-ffff-ffff-ffff-ffffffffffff");
    seed_conversation(&db, &archived_id);
    let mut archived = make_workspace(archived_id);
    archived.status = AgentConversationWorkspaceStatus::Archived;
    archived.publication_pr_number = Some(96);
    archived.publication_pr_status = Some("open".to_string());
    archived.publication_push_status = Some("refreshing".to_string());
    repo.create_or_update(archived.clone()).await.unwrap();
    set_workspace_updated_at(&db, &archived.conversation_id, stale);

    let workspaces = repo
        .list_active_transient_publish_status_workspaces(300)
        .await
        .unwrap();

    assert_eq!(
        workspaces
            .into_iter()
            .map(|workspace| workspace.conversation_id)
            .collect::<Vec<_>>(),
        vec![describing.conversation_id, refreshing.conversation_id]
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
async fn auto_publish_preferences_round_trip() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    repo.create_or_update(workspace).await.unwrap();

    repo.update_auto_publish_preferences(
        &conversation_id,
        false,
        Some(true),
        Some(true),
        false,
        false,
        Some("paused"),
        Some("Auto Publish is paused."),
    )
    .await
    .unwrap();

    let updated = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert!(!updated.auto_publish_enabled);
    assert_eq!(updated.auto_publish_paused_pr_autofix_enabled, Some(true));
    assert_eq!(
        updated.auto_publish_paused_pr_auto_merge_desired,
        Some(true)
    );
    assert!(!updated.pr_autofix_enabled);
    assert!(!updated.pr_auto_merge_desired);
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("paused"));
}

#[tokio::test]
async fn auto_publish_initial_pr_preference_round_trip() {
    let (_db, repo, conversation_id) = setup_repo();
    let workspace = make_workspace(conversation_id.clone());
    repo.create_or_update(workspace).await.unwrap();

    let loaded = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert!(!loaded.auto_publish_initial_pr_enabled);

    repo.update_auto_publish_initial_pr_preference(&conversation_id, true)
        .await
        .unwrap();

    let updated = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert!(updated.auto_publish_initial_pr_enabled);
    assert!(updated.auto_publish_enabled);
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
