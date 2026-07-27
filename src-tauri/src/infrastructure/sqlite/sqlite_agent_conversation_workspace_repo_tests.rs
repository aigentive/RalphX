use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentConversationWorkspacePublicationEvent,
    AgentConversationWorkspaceStatus, AgentWorkspaceFollowupProvenance,
    AgentWorkspacePrCommentEvidenceUpsert, AgentWorkspacePrDescription,
    AgentWorkspacePrMetadataDecision, AgentWorkspacePrReviewAction,
    AgentWorkspacePrReviewActionKind, AgentWorkspacePrReviewActionStatus,
    AgentWorkspacePrReviewMonitor, AgentWorkspacePrReviewMonitorStatus,
    AgentWorkspaceReviewApprovalSnapshot, AgentWorkspaceReviewAutoMergeGuard,
    AgentWorkspaceReviewAutoMergeGuardStatus, AgentWorkspaceReviewFixerSnapshot,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewHunkAnnotation,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, AgentWorkspaceSourcePullRequest, ArtifactId,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranchId, ProjectId,
    DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceLocalCleanupClaim,
    AgentWorkspacePrReviewActionMutation, AgentWorkspaceRepairStateGuard,
    AgentWorkspaceRepairStateTransition,
};
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

#[tokio::test]
async fn publication_pushed_sha_setter_is_branch_fenced_and_restart_clears_it() {
    let (_db, repo, conversation_id) = setup_repo();
    let workspace = repo
        .create_or_update(make_workspace(conversation_id))
        .await
        .unwrap();

    assert!(repo
        .set_publication_pushed_sha(
            &conversation_id,
            &workspace.branch_name,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .await
        .unwrap());
    assert!(!repo
        .set_publication_pushed_sha(
            &conversation_id,
            "stale-branch",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
        .await
        .unwrap());
    repo.create_or_update(workspace.clone()).await.unwrap();
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .publication_pushed_sha
            .as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );

    repo.restore_after_restart(
        &conversation_id,
        &IdeationSessionId::from_string("session-restart"),
        &PlanBranchId::from_string("plan-branch-restart"),
    )
    .await
    .unwrap();
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .publication_pushed_sha,
        None
    );
}

#[tokio::test]
async fn workspace_review_fixer_claim_is_exact_and_single_winner() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id))
        .await
        .unwrap();
    let artifact_id = ArtifactId::from_string("artifact-fixer-claim");
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id,
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("diff-claim".to_string());
    monitor.reviewed_diff_fingerprint = Some("diff-claim".to_string());
    monitor.review_artifact_id = Some(artifact_id.clone());
    monitor.review_artifact_version = Some(4);
    monitor.review_blocking_fingerprint = Some("blocker-claim".to_string());
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();
    let snapshot = AgentWorkspaceReviewFixerSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-claim".to_string(),
        artifact_id,
        artifact_version: 4,
        blocking_fingerprint: "blocker-claim".to_string(),
    };

    let claimed = repo
        .claim_workspace_review_fixer(
            &conversation_id,
            &snapshot,
            "attempt-one",
            chrono::Utc::now(),
        )
        .await
        .unwrap()
        .expect("exact snapshot should win");
    assert_eq!(
        claimed.review_fixer_attempt_id.as_deref(),
        Some("attempt-one")
    );
    assert!(repo
        .claim_workspace_review_fixer(
            &conversation_id,
            &snapshot,
            "attempt-two",
            chrono::Utc::now(),
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn workspace_review_fixer_settlement_rejects_refreshed_target_authority() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id))
        .await
        .unwrap();
    let artifact_id = ArtifactId::from_string("artifact-fixer-stale-settle");
    let snapshot = AgentWorkspaceReviewFixerSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-old".to_string(),
        artifact_id: artifact_id.clone(),
        artifact_version: 4,
        blocking_fingerprint: "blocker-old".to_string(),
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id,
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.current_target_scope = Some(snapshot.target_scope);
    monitor.reviewed_target_scope = Some(snapshot.target_scope);
    monitor.current_diff_fingerprint = Some(snapshot.diff_fingerprint.clone());
    monitor.reviewed_diff_fingerprint = Some(snapshot.diff_fingerprint.clone());
    monitor.review_artifact_id = Some(artifact_id);
    monitor.review_artifact_version = Some(snapshot.artifact_version);
    monitor.review_blocking_fingerprint = Some(snapshot.blocking_fingerprint.clone());
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();
    let mut claimed = repo
        .claim_workspace_review_fixer(
            &conversation_id,
            &snapshot,
            "attempt-stale",
            chrono::Utc::now(),
        )
        .await
        .unwrap()
        .unwrap();

    let mut refreshed = claimed.clone();
    refreshed.current_diff_fingerprint = Some("diff-new".to_string());
    refreshed.reviewed_diff_fingerprint = Some("diff-new".to_string());
    refreshed.review_blocking_fingerprint = Some("blocker-new".to_string());
    repo.upsert_workspace_review_monitor(refreshed.clone())
        .await
        .unwrap();
    claimed.review_fixer_status = Some("running".to_string());

    assert!(repo
        .settle_workspace_review_fixer_attempt(claimed, "attempt-stale", &snapshot)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        repo.get_workspace_review_monitor(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .current_diff_fingerprint,
        refreshed.current_diff_fingerprint
    );
}

#[tokio::test]
async fn invalid_workspace_review_fixer_attempt_failure_is_attempt_scoped() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id))
        .await
        .unwrap();
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id,
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = monitor.current_target_scope;
    monitor.current_diff_fingerprint = Some("diff-current".to_string());
    monitor.reviewed_diff_fingerprint = monitor.current_diff_fingerprint.clone();
    monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-current"));
    monitor.review_artifact_version = Some(1);
    monitor.review_blocking_fingerprint = Some("   ".to_string());
    monitor.review_fixer_status = Some("routing".to_string());
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();

    assert!(repo
        .fail_invalid_workspace_review_fixer_attempt(
            &conversation_id,
            Some("attempt-stale"),
            "invalid authority",
        )
        .await
        .unwrap()
        .is_none());
    let failed = repo
        .fail_invalid_workspace_review_fixer_attempt(&conversation_id, None, "invalid authority")
        .await
        .unwrap()
        .expect("the exact malformed attempt without an id should fail");
    assert_eq!(failed.review_fixer_status.as_deref(), Some("failed"));
    assert_eq!(failed.last_error.as_deref(), Some("invalid authority"));
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

fn seed_artifact(db: &SqliteTestDb, artifact_id: &ArtifactId, version: u32) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO artifacts (
                id, type, name, content_type, content_text, created_by, version, created_at
             ) VALUES (
                ?1, 'pr_review', 'Workspace Review', 'inline', 'Review body',
                'ralphx-workspace-reviewer', ?2, '2026-04-26T09:00:00Z'
             )",
            rusqlite::params![artifact_id.as_str(), i64::from(version)],
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

#[tokio::test]
async fn reserved_workspace_review_start_failure_is_exact_and_cannot_clobber_newer_run() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .expect("insert workspace");
    let review_conversation_id =
        ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("diff-new".to_string());
    monitor.review_conversation_id = Some(review_conversation_id.clone());
    monitor.last_run_id = Some("run-new".to_string());
    repo.upsert_workspace_review_monitor(monitor)
        .await
        .expect("insert reserved monitor");

    assert!(!repo
        .fail_reserved_workspace_review_start(
            &conversation_id,
            AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            "diff-new",
            &review_conversation_id,
            "run-old",
            "stale failure",
        )
        .await
        .expect("reject stale reservation"));
    let unchanged = repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("load unchanged monitor")
        .expect("monitor exists");
    assert_eq!(
        unchanged.status,
        AgentWorkspaceReviewMonitorStatus::Reviewing
    );
    assert_eq!(unchanged.last_run_id.as_deref(), Some("run-new"));
    assert!(unchanged.last_error.is_none());

    assert!(repo
        .fail_reserved_workspace_review_start(
            &conversation_id,
            AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            "diff-new",
            &review_conversation_id,
            "run-new",
            "launch failed",
        )
        .await
        .expect("fail exact reservation"));
    let failed = repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("load failed monitor")
        .expect("monitor exists");
    assert_eq!(failed.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        failed.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        failed.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert_eq!(failed.last_error.as_deref(), Some("launch failed"));
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
async fn repair_state_cas_is_null_safe_atomic_and_preserves_unrelated_fields() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_supervision_summary = Some("old blocker".to_string());
    workspace.pr_supervision_updated_at = None;
    workspace.publication_pr_url = Some("https://example.test/pr/9".to_string());
    repo.create_or_update(workspace.clone()).await.unwrap();

    let claimed_at = chrono::Utc::now();
    assert!(repo
        .compare_and_set_repair_state(
            &conversation_id,
            &AgentWorkspaceRepairStateGuard::from_workspace(&workspace),
            &AgentWorkspaceRepairStateTransition {
                publication_push_status: Some("needs_agent".to_string()),
                pr_supervision_status: Some("fixing".to_string()),
                pr_supervision_summary: Some("Repair is running.".to_string()),
                pr_supervision_updated_at: claimed_at,
                pr_auto_merge_current: Some(false),
                base_commit: Some("base-repaired".to_string()),
            },
        )
        .await
        .unwrap());
    let claimed = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.pr_supervision_status.as_deref(), Some("fixing"));
    assert_eq!(claimed.pr_auto_merge_current, Some(false));
    assert_eq!(claimed.base_commit.as_deref(), Some("base-repaired"));
    assert_eq!(claimed.publication_pr_url, workspace.publication_pr_url);

    let stale_guard = AgentWorkspaceRepairStateGuard {
        publication_push_status: Some("needs_agent".to_string()),
        pr_supervision_status: Some("fixing".to_string()),
        pr_supervision_updated_at: None,
    };
    assert!(!repo
        .compare_and_set_repair_state(
            &conversation_id,
            &stale_guard,
            &AgentWorkspaceRepairStateTransition {
                publication_push_status: Some("failed".to_string()),
                pr_supervision_status: Some("blocked".to_string()),
                pr_supervision_summary: Some("stale failure".to_string()),
                pr_supervision_updated_at: claimed_at + chrono::Duration::seconds(1),
                pr_auto_merge_current: Some(true),
                base_commit: Some("stale-base".to_string()),
            },
        )
        .await
        .unwrap());
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap(),
        claimed
    );
}

#[tokio::test]
async fn repair_state_and_events_transaction_rolls_back_on_insert_failure() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_supervision_updated_at = Some(chrono::Utc::now());
    repo.create_or_update(workspace.clone()).await.unwrap();
    let workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("failed".to_string()),
        pr_supervision_status: Some("blocked".to_string()),
        pr_supervision_summary: Some("Review handoff aborted.".to_string()),
        pr_supervision_updated_at: workspace.pr_supervision_updated_at.unwrap()
            + chrono::Duration::seconds(1),
        pr_auto_merge_current: None,
        base_commit: None,
    };
    let event = AgentConversationWorkspacePublicationEvent::new(
        conversation_id.clone(),
        "pr_autofix_workspace_review_aborted",
        "failed",
        "Review handoff aborted.",
        Some("workspace_review_aborted".to_string()),
    );
    let duplicate = event.clone();

    assert!(repo
        .compare_and_set_repair_state_with_events(
            &conversation_id,
            &AgentWorkspaceRepairStateGuard::from_workspace(&workspace),
            &transition,
            vec![event, duplicate],
        )
        .await
        .is_err());
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap(),
        workspace
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap()
        .is_empty());
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
    second.task_pipeline_session_id = Some(session_id.clone());
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

    let task_pipeline = repo
        .get_by_task_pipeline_session_id(&session_id)
        .await
        .unwrap()
        .expect("durably attached Tasks workspace should load");
    assert_eq!(task_pipeline.conversation_id, second_id);

    let missing = repo
        .get_by_linked_ideation_session_id(&IdeationSessionId::from_string("missing-session"))
        .await
        .unwrap();
    assert!(missing.is_none());
    assert!(repo
        .get_by_task_pipeline_session_id(&IdeationSessionId::from_string("missing-session"))
        .await
        .unwrap()
        .is_none());
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
async fn local_cleanup_status_can_be_read_and_cleared() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();

    repo.mark_local_cleanup_status(&conversation_id, "workspace_dirty", chrono::Utc::now())
        .await
        .unwrap();

    assert_eq!(
        repo.get_local_cleanup_status(&conversation_id)
            .await
            .unwrap()
            .as_deref(),
        Some("workspace_dirty")
    );

    repo.clear_local_cleanup_status(&conversation_id)
        .await
        .unwrap();

    assert_eq!(
        repo.get_local_cleanup_status(&conversation_id)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn local_cleanup_claim_is_atomic_and_finalize_requires_cleaning() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .expect("insert workspace");
    let repo = std::sync::Arc::new(repo);
    let claimed_at = chrono::Utc::now();
    let stale_before = claimed_at - chrono::Duration::hours(1);

    let (first, second) = tokio::join!(
        repo.claim_local_cleanup(&conversation_id, claimed_at, stale_before),
        repo.claim_local_cleanup(&conversation_id, claimed_at, stale_before),
    );
    let claims = [first.expect("first claim"), second.expect("second claim")];
    assert_eq!(
        claims
            .iter()
            .filter(|claim| **claim == AgentWorkspaceLocalCleanupClaim::Claimed)
            .count(),
        1
    );
    assert!(claims.contains(&AgentWorkspaceLocalCleanupClaim::AlreadyInProgress));

    let replacement_claimed_at = claimed_at + chrono::Duration::hours(2);
    assert_eq!(
        repo.claim_local_cleanup(
            &conversation_id,
            replacement_claimed_at,
            claimed_at + chrono::Duration::seconds(1),
        )
        .await
        .expect("replacement claim"),
        AgentWorkspaceLocalCleanupClaim::Claimed
    );
    assert!(!repo
        .finalize_local_cleanup(
            &conversation_id,
            claimed_at,
            "failed_unsafe",
            chrono::Utc::now(),
        )
        .await
        .expect("stale owner finalize is rejected"));
    assert!(repo
        .finalize_local_cleanup(
            &conversation_id,
            replacement_claimed_at,
            "cleaned",
            chrono::Utc::now(),
        )
        .await
        .expect("replacement owner finalizes"));
    assert_eq!(
        repo.claim_local_cleanup(&conversation_id, chrono::Utc::now(), stale_before)
            .await
            .expect("claim after success"),
        AgentWorkspaceLocalCleanupClaim::AlreadyCleaned
    );
}

#[tokio::test]
async fn terminal_cleanup_candidates_retry_markers_without_timestamps() {
    let (db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.publication_pr_number = Some(73);
    workspace.publication_pr_status = Some("closed".to_string());
    repo.create_or_update(workspace).await.unwrap();
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE agent_conversation_workspaces
             SET local_cleanup_status = 'failed_operational', local_cleanup_checked_at = NULL
             WHERE conversation_id = ?1",
            rusqlite::params![conversation_id.as_str()],
        )
        .unwrap();
    });

    let candidates = repo
        .get_terminal_local_cleanup_candidates_by_project_id(&ProjectId::from_string(
            "project-1".to_string(),
        ))
        .await
        .expect("candidate lookup");

    assert_eq!(candidates.len(), 1);
}

#[tokio::test]
async fn restore_after_restart_reactivates_links_and_clears_cleanup_marker() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.status = AgentConversationWorkspaceStatus::Missing;
    repo.create_or_update(workspace).await.unwrap();
    repo.mark_local_cleanup_status(&conversation_id, "cleaned", chrono::Utc::now())
        .await
        .unwrap();
    let session_id = IdeationSessionId::from_string("restart-session");
    let plan_branch_id = PlanBranchId::from_string("restart-plan-branch");

    repo.restore_after_restart(&conversation_id, &session_id, &plan_branch_id)
        .await
        .unwrap();

    let restored = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should remain persisted");
    assert_eq!(restored.status, AgentConversationWorkspaceStatus::Active);
    assert_eq!(
        restored.linked_ideation_session_id.as_ref(),
        Some(&session_id)
    );
    assert_eq!(
        restored.linked_plan_branch_id.as_ref(),
        Some(&plan_branch_id)
    );
    assert_eq!(
        repo.get_local_cleanup_status(&conversation_id)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn restore_after_restart_rejects_a_missing_workspace() {
    let (_db, repo, _) = setup_repo();
    let missing_conversation_id = ChatConversationId::new();
    let error = repo
        .restore_after_restart(
            &missing_conversation_id,
            &IdeationSessionId::new(),
            &PlanBranchId::new(),
        )
        .await
        .expect_err("restart repair must not succeed without a workspace row");

    assert!(error.to_string().contains("Workspace not found"));
}

#[tokio::test]
async fn terminal_cleanup_candidates_retry_unsafe_after_ttl() {
    let (db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id);
    workspace.publication_pr_number = Some(80);
    workspace.publication_pr_status = Some("closed".to_string());
    repo.create_or_update(workspace).await.unwrap();

    let old_timestamp = chrono::Utc::now() - chrono::Duration::hours(2);
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
        "unsafe after retry window should be retryable"
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
        "unsafe before retry window should not be retryable"
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

    let old_timestamp = chrono::Utc::now() - chrono::Duration::hours(2);
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
        "target_ref_missing after retry window should be retryable"
    );

    let _ = db;
}

#[tokio::test]
async fn terminal_cleanup_candidates_retry_workspace_dirty_after_retry_window() {
    let (db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id);
    workspace.publication_pr_number = Some(82);
    workspace.publication_pr_status = Some("closed".to_string());
    repo.create_or_update(workspace).await.unwrap();

    let old_timestamp = chrono::Utc::now() - chrono::Duration::hours(2);
    repo.mark_local_cleanup_status(&conversation_id, "workspace_dirty", old_timestamp)
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
        "workspace_dirty after retry window should be retryable"
    );

    let recent_timestamp = chrono::Utc::now();
    repo.mark_local_cleanup_status(&conversation_id, "workspace_dirty", recent_timestamp)
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
        "workspace_dirty before retry window should not be retryable"
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
async fn pr_metadata_decisions_decode_legacy_reject_invalid_and_clear() {
    let (db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();
    let cases = [
        AgentWorkspacePrMetadataDecision::Preserve,
        AgentWorkspacePrMetadataDecision::patch(Some("title".to_string()), None).unwrap(),
        AgentWorkspacePrMetadataDecision::patch(None, Some("body".to_string())).unwrap(),
        AgentWorkspacePrMetadataDecision::patch(
            Some("title".to_string()),
            Some("body".to_string()),
        )
        .unwrap(),
    ];
    for decision in cases {
        repo.save_pr_metadata_decision(&conversation_id, decision.clone())
            .await
            .unwrap();
        assert_eq!(
            repo.get_pr_metadata_decision(&conversation_id)
                .await
                .unwrap(),
            Some(decision)
        );
    }

    let id = conversation_id.as_str().to_string();
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE agent_conversation_workspaces SET publication_pr_metadata_decision = NULL, publication_pr_title = 'legacy title', publication_pr_body = 'legacy body' WHERE conversation_id = ?1",
            rusqlite::params![id],
        )
        .unwrap();
    });
    assert_eq!(
        repo.get_pr_metadata_decision(&conversation_id)
            .await
            .unwrap(),
        Some(AgentWorkspacePrMetadataDecision::Patch {
            title: Some("legacy title".to_string()),
            body_markdown: Some("legacy body".to_string()),
        })
    );

    let id = conversation_id.as_str().to_string();
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE agent_conversation_workspaces SET publication_pr_metadata_decision = 'invalid' WHERE conversation_id = ?1",
            rusqlite::params![id],
        )
        .unwrap();
    });
    assert!(repo
        .get_pr_metadata_decision(&conversation_id)
        .await
        .is_err());

    repo.clear_pr_metadata_decision(&conversation_id)
        .await
        .unwrap();
    assert_eq!(
        repo.get_pr_metadata_decision(&conversation_id)
            .await
            .unwrap(),
        None
    );
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
    let guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 483,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::SelectedSource,
        diff_fingerprint: "fingerprint".to_string(),
        head_sha: Some("head-sha".to_string()),
        last_error: None,
    };
    monitor.auto_merge_guard = Some(guard.clone());

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
    assert_eq!(saved.auto_merge_guard.as_ref(), Some(&guard));

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
    assert_eq!(updated.auto_merge_guard.as_ref(), Some(&guard));

    let loaded = repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .unwrap()
        .expect("monitor should load");
    assert_eq!(loaded, updated);

    let stale_guard = AgentWorkspaceReviewAutoMergeGuard {
        last_error: Some("stale writer".to_string()),
        ..guard.clone()
    };
    assert!(!repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &conversation_id,
            Some(stale_guard),
            None,
        )
        .await
        .unwrap());
    let restoring_guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        ..guard.clone()
    };
    assert!(repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &conversation_id,
            Some(guard),
            Some(restoring_guard.clone()),
        )
        .await
        .unwrap());
    assert_eq!(
        repo.get_workspace_review_monitor(&conversation_id)
            .await
            .unwrap()
            .expect("monitor should remain")
            .auto_merge_guard,
        Some(restoring_guard)
    );
}

#[tokio::test]
async fn workspace_review_approval_cas_and_audit_event_commit_exactly_once() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();
    let artifact_id = ArtifactId::from_string("artifact-bypass");
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("diff-current".to_string());
    monitor.reviewed_diff_fingerprint = Some("diff-current".to_string());
    monitor.review_artifact_id = Some(artifact_id.clone());
    monitor.review_artifact_version = Some(3);
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();

    let stale_snapshot = AgentWorkspaceReviewApprovalSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-stale".to_string(),
        artifact_id: artifact_id.clone(),
        artifact_version: 3,
    };
    assert!(repo
        .approve_workspace_review_anyway(&conversation_id, &stale_snapshot, chrono::Utc::now(),)
        .await
        .unwrap()
        .is_none());
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap()
        .is_empty());

    let snapshot = AgentWorkspaceReviewApprovalSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-current".to_string(),
        artifact_id,
        artifact_version: 3,
    };
    let approved = repo
        .approve_workspace_review_anyway(&conversation_id, &snapshot, chrono::Utc::now())
        .await
        .unwrap()
        .expect("exact snapshot should apply");
    assert_eq!(
        approved.review_gate_status,
        AgentWorkspaceReviewGateStatus::Passed
    );
    assert_eq!(
        approved.review_outcome,
        AgentWorkspaceReviewOutcome::Blocking
    );

    assert!(repo
        .approve_workspace_review_anyway(&conversation_id, &snapshot, chrono::Utc::now())
        .await
        .unwrap()
        .is_none());
    let events = repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].step, "workspace_review_approved_anyway");
    assert!(events[0].summary.contains("diff-current"));
    assert_eq!(
        events[0].classification.as_deref(),
        Some("workspace_review_approved_anyway")
    );
}

#[tokio::test]
async fn workspace_review_approval_rejects_active_publish_status_without_audit_event() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.publication_push_status = Some("checking".to_string());
    repo.create_or_update(workspace).await.unwrap();
    let artifact_id = ArtifactId::from_string("artifact-bypass-publishing");
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("diff-publishing".to_string());
    monitor.reviewed_diff_fingerprint = Some("diff-publishing".to_string());
    monitor.review_artifact_id = Some(artifact_id.clone());
    monitor.review_artifact_version = Some(8);
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();

    let snapshot = AgentWorkspaceReviewApprovalSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-publishing".to_string(),
        artifact_id,
        artifact_version: 8,
    };
    assert!(repo
        .approve_workspace_review_anyway(&conversation_id, &snapshot, chrono::Utc::now())
        .await
        .unwrap()
        .is_none());
    let stored = repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .unwrap()
        .expect("monitor should remain");
    assert_eq!(
        stored.review_gate_status,
        AgentWorkspaceReviewGateStatus::Blocking
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn list_reviewing_workspace_review_monitors_returns_only_running_reviews() {
    let (db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();

    let ready_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &ready_id);
    repo.create_or_update(make_workspace(ready_id.clone()))
        .await
        .unwrap();

    let mut reviewing = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    reviewing.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    reviewing.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    repo.upsert_workspace_review_monitor(reviewing)
        .await
        .unwrap();

    let mut ready =
        AgentWorkspaceReviewMonitor::new(ready_id, ProjectId::from_string("project-1".to_string()));
    ready.status = AgentWorkspaceReviewMonitorStatus::Ready;
    ready.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    ready.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    repo.upsert_workspace_review_monitor(ready).await.unwrap();

    let monitors = repo
        .list_reviewing_workspace_review_monitors()
        .await
        .unwrap();

    assert_eq!(monitors.len(), 1);
    assert_eq!(monitors[0].conversation_id, conversation_id);
    assert_eq!(
        monitors[0].status,
        AgentWorkspaceReviewMonitorStatus::Reviewing
    );
}

#[tokio::test]
async fn complete_workspace_review_auto_merge_restore_clears_guard_atomically() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    workspace.publication_pr_number = Some(42);
    repo.create_or_update(workspace).await.unwrap();

    let restoring_guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: None,
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("workspace-delta".to_string());
    monitor.auto_merge_guard = Some(restoring_guard.clone());
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();

    assert!(repo
        .complete_workspace_review_auto_merge_restore(&conversation_id, restoring_guard)
        .await
        .unwrap());

    let loaded_workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should load");
    assert_eq!(loaded_workspace.pr_auto_merge_current, Some(true));
    assert_eq!(
        loaded_workspace.pr_supervision_status.as_deref(),
        Some("monitoring")
    );
    assert!(repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .unwrap()
        .expect("monitor should load")
        .auto_merge_guard
        .is_none());
}

#[tokio::test]
async fn complete_workspace_review_auto_merge_restore_accepts_current_selected_source() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    workspace.publication_pr_number = Some(7);
    workspace.publication_pr_status = Some("merged".to_string());
    repo.create_or_update(workspace).await.unwrap();

    let restoring_guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::SelectedSource,
        diff_fingerprint: "selected-source".to_string(),
        head_sha: Some("reviewed-head".to_string()),
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.current_diff_fingerprint = Some("selected-source".to_string());
    monitor.selected_source_pull_request_number = Some(42);
    monitor.selected_source_head_sha = Some("reviewed-head".to_string());
    monitor.auto_merge_guard = Some(restoring_guard.clone());
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();

    assert!(repo
        .complete_workspace_review_auto_merge_restore(&conversation_id, restoring_guard)
        .await
        .unwrap());
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should load")
            .pr_auto_merge_current,
        Some(true)
    );
    assert!(repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .unwrap()
        .expect("monitor should load")
        .auto_merge_guard
        .is_none());
}

#[tokio::test]
async fn complete_workspace_review_auto_merge_restore_accepts_refreshed_workspace_delta() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    workspace.publication_pr_number = Some(42);
    repo.create_or_update(workspace).await.unwrap();

    let restoring_guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: None,
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("new-workspace-delta".to_string());
    monitor.auto_merge_guard = Some(restoring_guard.clone());
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();

    assert!(repo
        .complete_workspace_review_auto_merge_restore(&conversation_id, restoring_guard.clone())
        .await
        .unwrap());

    let loaded_workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should load");
    assert_eq!(loaded_workspace.pr_auto_merge_current, Some(true));
    assert!(repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .unwrap()
        .expect("monitor should load")
        .auto_merge_guard
        .is_none());
}

#[tokio::test]
async fn complete_workspace_review_auto_merge_restore_rejects_missing_publication_pr() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    repo.create_or_update(workspace).await.unwrap();

    let restoring_guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: None,
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("workspace-delta".to_string());
    monitor.auto_merge_guard = Some(restoring_guard.clone());
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();

    assert!(!repo
        .complete_workspace_review_auto_merge_restore(&conversation_id, restoring_guard.clone())
        .await
        .unwrap());
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should load")
            .pr_auto_merge_current,
        Some(false)
    );
    assert_eq!(
        repo.get_workspace_review_monitor(&conversation_id)
            .await
            .unwrap()
            .expect("monitor should load")
            .auto_merge_guard,
        Some(restoring_guard)
    );
}

#[tokio::test]
async fn complete_workspace_review_auto_merge_restore_rejects_stale_selected_source_head() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    repo.create_or_update(workspace).await.unwrap();

    let restoring_guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::SelectedSource,
        diff_fingerprint: "selected-source".to_string(),
        head_sha: Some("reviewed-head".to_string()),
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.current_diff_fingerprint = Some("selected-source".to_string());
    monitor.selected_source_pull_request_number = Some(42);
    monitor.selected_source_head_sha = Some("new-head".to_string());
    monitor.auto_merge_guard = Some(restoring_guard.clone());
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();

    assert!(!repo
        .complete_workspace_review_auto_merge_restore(&conversation_id, restoring_guard.clone())
        .await
        .unwrap());
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should load")
            .pr_auto_merge_current,
        Some(false)
    );
    assert_eq!(
        repo.get_workspace_review_monitor(&conversation_id)
            .await
            .unwrap()
            .expect("monitor should load")
            .auto_merge_guard,
        Some(restoring_guard)
    );
}

#[tokio::test]
async fn complete_workspace_review_auto_merge_restore_rejects_terminal_guarded_pr() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("merged".to_string());
    repo.create_or_update(workspace).await.unwrap();

    let restoring_guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: None,
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("workspace-delta".to_string());
    monitor.auto_merge_guard = Some(restoring_guard.clone());
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();

    assert!(!repo
        .complete_workspace_review_auto_merge_restore(&conversation_id, restoring_guard.clone())
        .await
        .unwrap());
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should load")
            .pr_auto_merge_current,
        Some(false)
    );
    assert_eq!(
        repo.get_workspace_review_monitor(&conversation_id)
            .await
            .unwrap()
            .expect("monitor should load")
            .auto_merge_guard,
        Some(restoring_guard)
    );
}

#[tokio::test]
async fn complete_workspace_review_auto_merge_restore_rejects_retargeted_publication_pr() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    workspace.publication_pr_number = Some(84);
    workspace.publication_pr_status = Some("open".to_string());
    repo.create_or_update(workspace).await.unwrap();

    let restoring_guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: None,
        last_error: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("workspace-delta".to_string());
    monitor.auto_merge_guard = Some(restoring_guard.clone());
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();

    assert!(!repo
        .complete_workspace_review_auto_merge_restore(&conversation_id, restoring_guard.clone())
        .await
        .unwrap());
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should load")
            .pr_auto_merge_current,
        Some(false)
    );
    assert_eq!(
        repo.get_workspace_review_monitor(&conversation_id)
            .await
            .unwrap()
            .expect("monitor should load")
            .auto_merge_guard,
        Some(restoring_guard)
    );
}

#[tokio::test]
async fn list_active_workspace_review_auto_merge_guards_returns_only_guarded_monitors() {
    let (db, repo, guarded_id) = setup_repo();
    repo.create_or_update(make_workspace(guarded_id.clone()))
        .await
        .unwrap();

    let unguarded_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &unguarded_id);
    repo.create_or_update(make_workspace(unguarded_id.clone()))
        .await
        .unwrap();

    let mut guarded = AgentWorkspaceReviewMonitor::new(
        guarded_id.clone(),
        ProjectId::from_string("project-1".to_string()),
    );
    guarded.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: None,
        last_error: None,
    });
    repo.upsert_workspace_review_monitor(guarded).await.unwrap();

    let unguarded = AgentWorkspaceReviewMonitor::new(
        unguarded_id,
        ProjectId::from_string("project-1".to_string()),
    );
    repo.upsert_workspace_review_monitor(unguarded)
        .await
        .unwrap();

    let guarded_monitors = repo
        .list_active_workspace_review_auto_merge_guards()
        .await
        .unwrap();
    assert_eq!(guarded_monitors.len(), 1);
    assert_eq!(guarded_monitors[0].conversation_id, guarded_id);
    assert_eq!(
        guarded_monitors[0]
            .auto_merge_guard
            .as_ref()
            .map(|guard| guard.status),
        Some(AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview)
    );
}

#[tokio::test]
async fn workspace_review_hunk_annotations_replace_per_artifact_version() {
    let (db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();
    let artifact_id = ArtifactId::from_string("artifact-current");
    seed_artifact(&db, &artifact_id, 2);

    let annotation = AgentWorkspaceReviewHunkAnnotation {
        id: "annotation-1".to_string(),
        conversation_id: conversation_id.clone(),
        project_id: ProjectId::from_string("project-1".to_string()),
        artifact_id: artifact_id.clone(),
        artifact_version: 2,
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        head_sha: Some("head-sha".to_string()),
        diff_fingerprint: "fingerprint".to_string(),
        path: "src/lib.rs".to_string(),
        diff_source: "committed".to_string(),
        hunk_header: "@@ -1,1 +1,2 @@".to_string(),
        old_start: 1,
        old_lines: 1,
        new_start: 1,
        new_lines: 2,
        title: Some("Updates lib".to_string()),
        message: "Explains the changed hunk.".to_string(),
        level: "notice".to_string(),
        created_by_run_id: Some("run-1".to_string()),
        created_at: chrono::Utc::now(),
    };

    repo.replace_workspace_review_hunk_annotations(
        &conversation_id,
        &artifact_id,
        vec![annotation.clone()],
    )
    .await
    .unwrap();
    let saved = repo
        .list_workspace_review_hunk_annotations(&conversation_id, &artifact_id)
        .await
        .unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id, annotation.id);
    assert_eq!(saved[0].path, "src/lib.rs");
    assert_eq!(saved[0].diff_source, "committed");
    assert_eq!(saved[0].artifact_version, 2);

    repo.replace_workspace_review_hunk_annotations(&conversation_id, &artifact_id, Vec::new())
        .await
        .unwrap();
    let replaced = repo
        .list_workspace_review_hunk_annotations(&conversation_id, &artifact_id)
        .await
        .unwrap();
    assert!(replaced.is_empty());
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
    assert!(saved.auto_approve_enabled);
    assert!(!saved.first_action_resolved);

    let settings_updated = repo
        .set_pr_review_auto_approve_enabled(&conversation_id, false)
        .await
        .unwrap();
    assert!(!settings_updated.auto_approve_enabled);
    let resolution_updated = repo
        .mark_pr_review_first_action_resolved(&conversation_id)
        .await
        .unwrap();
    assert!(resolution_updated.first_action_resolved);

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
    assert!(!loaded.auto_approve_enabled);

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
    assert!(!preserved.auto_approve_enabled);
    assert!(preserved.first_action_resolved);
    let missing_conversation_id = ChatConversationId::new();
    assert!(repo
        .set_pr_review_auto_approve_enabled(&missing_conversation_id, true)
        .await
        .is_err());
    assert!(repo
        .mark_pr_review_first_action_resolved(&missing_conversation_id)
        .await
        .is_err());
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
async fn pr_review_terminal_settlement_is_atomic_idempotent_and_supersedes_actionable_rows() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 267,
        url: Some("https://github.com/owner/repo/pull/267".to_string()),
        title: Some("Review target".to_string()),
        head_ref_name: "feature/review-target".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head-sha".to_string()),
    });
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_supervision_summary = Some("Waiting for review".to_string());
    repo.create_or_update(workspace).await.unwrap();

    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        267,
        Some("head-sha".to_string()),
    );
    monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
    repo.upsert_pr_review_monitor(monitor).await.unwrap();

    let pending = repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            267,
            "head-sha".to_string(),
            AgentWorkspacePrReviewActionKind::Approve,
            "Approve".to_string(),
            "Looks good".to_string(),
            None,
            Some("run-pending".to_string()),
        ))
        .await
        .unwrap();
    let submitting = repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            267,
            "older-head".to_string(),
            AgentWorkspacePrReviewActionKind::RequestChanges,
            "Request changes".to_string(),
            "Please fix this".to_string(),
            None,
            Some("run-submitting".to_string()),
        ))
        .await
        .unwrap();
    assert!(repo
        .claim_pending_pr_review_action(&submitting.id)
        .await
        .unwrap());
    let historical = repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            267,
            "historical-head".to_string(),
            AgentWorkspacePrReviewActionKind::Comment,
            "Historical".to_string(),
            "Already submitted".to_string(),
            None,
            Some("run-historical".to_string()),
        ))
        .await
        .unwrap();
    repo.update_pr_review_action_status(
        &historical.id,
        AgentWorkspacePrReviewActionStatus::Submitted,
        Some("review-1"),
    )
    .await
    .unwrap();

    let settled = repo
        .settle_pr_review_terminal(&conversation_id, 267, "merged", "Pull request merged")
        .await
        .unwrap();
    assert!(settled.event_inserted);
    assert_eq!(
        settled
            .superseded_action_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>(),
        [pending.id.clone(), submitting.id.clone()]
            .into_iter()
            .collect()
    );

    let workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.publication_pr_status.as_deref(), Some("merged"));
    assert!(workspace.pr_supervision_status.is_none());
    assert!(workspace.pr_supervision_summary.is_none());
    let monitor = repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        monitor.status,
        AgentWorkspacePrReviewMonitorStatus::Terminal
    );
    assert!(!monitor.monitor_enabled);
    assert_eq!(monitor.last_review_outcome.as_deref(), Some("merged"));
    assert!(monitor.last_error.is_none());
    assert_eq!(
        repo.get_pr_review_action(&pending.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkspacePrReviewActionStatus::Superseded
    );
    assert_eq!(
        repo.get_pr_review_action(&submitting.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkspacePrReviewActionStatus::Superseded
    );
    assert_eq!(
        repo.get_pr_review_action(&historical.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkspacePrReviewActionStatus::Submitted
    );

    let repeated = repo
        .settle_pr_review_terminal(&conversation_id, 267, "merged", "Pull request merged")
        .await
        .unwrap();
    assert!(!repeated.event_inserted);
    let mut superseded_action_ids = repeated.superseded_action_ids;
    superseded_action_ids.sort();
    let mut expected_action_ids = vec![pending.id.clone(), submitting.id.clone()];
    expected_action_ids.sort();
    assert_eq!(superseded_action_ids, expected_action_ids);
    assert_eq!(
        repo.list_publication_events(&conversation_id)
            .await
            .unwrap()
            .iter()
            .filter(|event| event.step == "pr_merged")
            .count(),
        1
    );
}

#[tokio::test]
async fn pr_review_lifecycle_listing_includes_paused_nonterminal_monitors() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 268,
        url: None,
        title: None,
        head_ref_name: "paused/head".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head".to_string()),
    });
    repo.create_or_update(workspace).await.unwrap();
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        268,
        Some("head".to_string()),
    );
    monitor.monitor_enabled = false;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Paused;
    repo.upsert_pr_review_monitor(monitor).await.unwrap();

    assert!(repo
        .list_active_pr_review_monitors()
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        repo.list_pr_review_lifecycle_monitors()
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn guarded_pr_review_transition_is_atomic_and_rejects_terminal_authority() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 269,
        url: None,
        title: None,
        head_ref_name: "guarded/head".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head".to_string()),
    });
    repo.create_or_update(workspace).await.unwrap();
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        269,
        Some("head".to_string()),
    );
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    monitor = repo.upsert_pr_review_monitor(monitor).await.unwrap();
    let action = AgentWorkspacePrReviewAction::new(
        conversation_id.clone(),
        269,
        "head".to_string(),
        AgentWorkspacePrReviewActionKind::Approve,
        "Approve".to_string(),
        "Looks good".to_string(),
        None,
        Some("run-guarded".to_string()),
    );
    monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
    let proposed = repo
        .transition_pr_review_state_if_nonterminal(
            monitor,
            Some(AgentWorkspacePrReviewActionMutation::UpsertPending(
                action.clone(),
            )),
        )
        .await
        .unwrap()
        .expect("proposal transition should commit");
    assert_eq!(
        proposed.action.as_ref().unwrap().status,
        AgentWorkspacePrReviewActionStatus::Pending
    );

    let mut submitting_monitor = proposed.monitor;
    submitting_monitor.status = AgentWorkspacePrReviewMonitorStatus::Submitting;
    let claimed = repo
        .transition_pr_review_state_if_nonterminal(
            submitting_monitor,
            Some(AgentWorkspacePrReviewActionMutation::CompareAndSet {
                action_id: proposed.action.as_ref().unwrap().id.clone(),
                expected: AgentWorkspacePrReviewActionStatus::Pending,
                status: AgentWorkspacePrReviewActionStatus::Submitting,
                submitted_review_id: None,
            }),
        )
        .await
        .unwrap()
        .expect("claim transition should commit");
    repo.settle_pr_review_terminal(&conversation_id, 269, "closed", "Closed")
        .await
        .unwrap();
    let stale = repo
        .transition_pr_review_state_if_nonterminal(
            claimed.monitor,
            Some(AgentWorkspacePrReviewActionMutation::CompareAndSet {
                action_id: claimed.action.unwrap().id,
                expected: AgentWorkspacePrReviewActionStatus::Submitting,
                status: AgentWorkspacePrReviewActionStatus::Submitted,
                submitted_review_id: Some("late-review".to_string()),
            }),
        )
        .await
        .unwrap();
    assert!(stale.is_none());
    assert_eq!(
        repo.get_pr_review_monitor(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkspacePrReviewMonitorStatus::Terminal
    );
    assert_eq!(
        repo.get_pr_review_action(&action.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkspacePrReviewActionStatus::Superseded
    );
}

#[tokio::test]
async fn terminal_monitor_rearms_only_after_live_open_and_rejects_terminal_settings() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 270,
        url: None,
        title: None,
        head_ref_name: "feature/rearm".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head".to_string()),
    });
    repo.create_or_update(workspace).await.unwrap();

    assert!(repo
        .settle_pr_review_terminal(&conversation_id, 270, "invalid", "Invalid")
        .await
        .is_err());
    let settled = repo
        .settle_pr_review_terminal(&conversation_id, 270, "merged", "Merged")
        .await
        .unwrap();
    assert!(settled.superseded_action_ids.is_empty());
    assert!(repo
        .rearm_terminal_pr_review_monitor_after_live_open(&conversation_id, 270)
        .await
        .unwrap()
        .is_none());
    assert!(repo
        .set_pr_review_auto_approve_enabled(&conversation_id, true)
        .await
        .is_err());
    assert!(repo
        .set_pr_review_monitor_enabled(&conversation_id, true)
        .await
        .is_err());

    repo.update_publication(
        &conversation_id,
        Some(270),
        None,
        Some("open"),
        Some("pushed"),
    )
    .await
    .unwrap();
    let rearmed = repo
        .rearm_terminal_pr_review_monitor_after_live_open(&conversation_id, 270)
        .await
        .unwrap()
        .expect("live open observation rearms terminal monitor");
    assert_eq!(
        rearmed.status,
        AgentWorkspacePrReviewMonitorStatus::Watching
    );
    assert!(rearmed.monitor_enabled);
    assert!(rearmed.last_error.is_none());
}

#[tokio::test]
async fn guarded_action_mutations_require_live_review_pr_authority() {
    let (_db, repo, conversation_id) = setup_repo();
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 271,
        url: None,
        title: None,
        head_ref_name: "feature/guarded-actions".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head".to_string()),
    });
    repo.create_or_update(workspace).await.unwrap();

    let action = AgentWorkspacePrReviewAction::new(
        conversation_id.clone(),
        271,
        "head".to_string(),
        AgentWorkspacePrReviewActionKind::Approve,
        "Approve".to_string(),
        "Looks good".to_string(),
        None,
        Some("run-guarded".to_string()),
    );
    let saved = repo
        .create_or_update_pr_review_action_if_nonterminal(action.clone())
        .await
        .unwrap();
    assert!(!repo
        .claim_pending_pr_review_action_if_nonterminal(&saved.id, &conversation_id, 999)
        .await
        .unwrap());
    assert!(repo
        .claim_pending_pr_review_action_if_nonterminal(&saved.id, &conversation_id, 271)
        .await
        .unwrap());

    repo.settle_pr_review_terminal(&conversation_id, 271, "closed", "Closed")
        .await
        .unwrap();
    assert!(repo
        .create_or_update_pr_review_action_if_nonterminal(AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            271,
            "new-head".to_string(),
            AgentWorkspacePrReviewActionKind::Approve,
            "Approve".to_string(),
            "Looks good".to_string(),
            None,
            Some("run-late".to_string()),
        ))
        .await
        .is_err());
    assert!(!repo
        .claim_pending_pr_review_action_if_nonterminal(&saved.id, &conversation_id, 271)
        .await
        .unwrap());
}

#[tokio::test]
async fn pr_review_monitor_rejects_stale_disabled_upserts_after_pause_and_restart() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();

    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        268,
        Some("head-a".to_string()),
    );
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    monitor.last_seen_head_sha = Some("authoritative-head".to_string());
    monitor.last_reviewed_head_sha = Some("authoritative-reviewed-head".to_string());
    monitor.last_review_outcome = Some("authoritative-outcome".to_string());
    monitor.review_artifact_head_sha = Some("authoritative-artifact-head".to_string());
    monitor.review_artifact_version = Some(2);
    monitor.updated_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    repo.upsert_pr_review_monitor(monitor.clone())
        .await
        .unwrap();

    let mut stale_disabled_callback = monitor;
    stale_disabled_callback.monitor_enabled = false;
    stale_disabled_callback.status = AgentWorkspacePrReviewMonitorStatus::Paused;
    stale_disabled_callback.last_seen_head_sha = Some("stale-head".to_string());
    stale_disabled_callback.last_reviewed_head_sha = Some("stale-reviewed-head".to_string());
    stale_disabled_callback.last_review_outcome = Some("stale-outcome".to_string());
    stale_disabled_callback.review_artifact_head_sha = Some("stale-artifact-head".to_string());
    stale_disabled_callback.review_artifact_version = Some(1);

    repo.set_pr_review_monitor_enabled(&conversation_id, false)
        .await
        .unwrap();

    let stale_write = repo
        .upsert_pr_review_monitor(stale_disabled_callback.clone())
        .await
        .unwrap();
    assert!(!stale_write.monitor_enabled);
    assert_eq!(
        stale_write.status,
        AgentWorkspacePrReviewMonitorStatus::Paused
    );
    assert_eq!(
        stale_write.last_seen_head_sha.as_deref(),
        Some("authoritative-head")
    );
    assert_eq!(
        stale_write.last_reviewed_head_sha.as_deref(),
        Some("authoritative-reviewed-head")
    );
    assert_eq!(
        stale_write.last_review_outcome.as_deref(),
        Some("authoritative-outcome")
    );
    assert_eq!(
        stale_write.review_artifact_head_sha.as_deref(),
        Some("authoritative-artifact-head")
    );
    assert_eq!(stale_write.review_artifact_version, Some(2));

    let restarted = repo
        .set_pr_review_monitor_enabled(&conversation_id, true)
        .await
        .unwrap();
    assert!(restarted.monitor_enabled);
    assert_eq!(
        restarted.status,
        AgentWorkspacePrReviewMonitorStatus::Watching
    );

    let stale_after_restart = repo
        .upsert_pr_review_monitor(stale_disabled_callback)
        .await
        .unwrap();
    assert!(stale_after_restart.monitor_enabled);
    assert_eq!(
        stale_after_restart.status,
        AgentWorkspacePrReviewMonitorStatus::Watching
    );
    assert_eq!(
        stale_after_restart.last_seen_head_sha.as_deref(),
        Some("authoritative-head")
    );
    assert_eq!(
        stale_after_restart.last_reviewed_head_sha.as_deref(),
        Some("authoritative-reviewed-head")
    );
    assert_eq!(
        stale_after_restart.last_review_outcome.as_deref(),
        Some("authoritative-outcome")
    );
    assert_eq!(
        stale_after_restart.review_artifact_head_sha.as_deref(),
        Some("authoritative-artifact-head")
    );
    assert_eq!(stale_after_restart.review_artifact_version, Some(2));
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

    assert!(repo
        .claim_pending_pr_review_action(&saved.id)
        .await
        .unwrap());
    assert!(!repo
        .claim_pending_pr_review_action(&saved.id)
        .await
        .unwrap());
    assert!(!repo
        .claim_pending_pr_review_action("missing-action")
        .await
        .unwrap());

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
async fn latest_pending_pr_review_action_is_deterministic_and_owner_scoped() {
    let (db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();

    let make_action = |owner: ChatConversationId, pr_number: i64, id: &str, head: &str| {
        let mut action = AgentWorkspacePrReviewAction::new(
            owner,
            pr_number,
            head.to_string(),
            AgentWorkspacePrReviewActionKind::RequestChanges,
            format!("Review {head}"),
            format!("Body for {head}"),
            None,
            Some(format!("run-{head}")),
        );
        action.id = id.to_string();
        action
    };

    let terminal = repo
        .create_or_update_pr_review_action(make_action(
            conversation_id.clone(),
            267,
            "terminal-action",
            "terminal-head",
        ))
        .await
        .unwrap();
    repo.update_pr_review_action_status(
        &terminal.id,
        AgentWorkspacePrReviewActionStatus::Submitted,
        Some("review-terminal"),
    )
    .await
    .unwrap();
    for (id, head) in [("tie-action-a", "head-a"), ("tie-action-b", "head-b")] {
        repo.create_or_update_pr_review_action(make_action(conversation_id.clone(), 267, id, head))
            .await
            .unwrap();
    }

    let other_conversation_id =
        ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    seed_conversation(&db, &other_conversation_id);
    repo.create_or_update(make_workspace(other_conversation_id.clone()))
        .await
        .unwrap();
    repo.create_or_update_pr_review_action(make_action(
        other_conversation_id.clone(),
        267,
        "other-owner-action",
        "other-owner-head",
    ))
    .await
    .unwrap();
    repo.create_or_update_pr_review_action(make_action(
        conversation_id.clone(),
        268,
        "other-pr-action",
        "other-pr-head",
    ))
    .await
    .unwrap();

    db.with_connection(|conn| {
        conn.execute(
            "UPDATE agent_workspace_pr_review_actions
             SET created_at = '2026-07-20T12:00:00Z',
                 updated_at = '2026-07-20T12:00:00Z'
             WHERE id IN ('tie-action-a', 'tie-action-b')",
            [],
        )
        .unwrap();
    });

    let selected = repo
        .get_latest_pending_pr_review_action(&conversation_id, 267)
        .await
        .expect("read latest pending action")
        .expect("pending action exists");

    assert_eq!(selected.id, "tie-action-b");
    assert_ne!(selected.id, terminal.id);
    assert!(repo
        .get_latest_pending_pr_review_action(&other_conversation_id, 268)
        .await
        .expect("read isolated owner")
        .is_none());
}

#[tokio::test]
async fn supersede_pending_pr_review_actions_except_head_keeps_current_and_terminal_actions() {
    let (_db, repo, conversation_id) = setup_repo();
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();

    let stale = repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            267,
            "old-head".to_string(),
            AgentWorkspacePrReviewActionKind::RequestChanges,
            "Old blocking issues".to_string(),
            "Please address old issues.".to_string(),
            None,
            Some("run-old".to_string()),
        ))
        .await
        .unwrap();
    let current = repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            267,
            "current-head".to_string(),
            AgentWorkspacePrReviewActionKind::Approve,
            "Current head passes".to_string(),
            "Approved.".to_string(),
            None,
            Some("run-current".to_string()),
        ))
        .await
        .unwrap();
    let submitted = repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            267,
            "submitted-head".to_string(),
            AgentWorkspacePrReviewActionKind::RequestChanges,
            "Already submitted".to_string(),
            "Submitted.".to_string(),
            None,
            Some("run-submitted".to_string()),
        ))
        .await
        .unwrap();
    repo.update_pr_review_action_status(
        &submitted.id,
        AgentWorkspacePrReviewActionStatus::Submitted,
        Some("review-submitted"),
    )
    .await
    .unwrap();

    let superseded_ids = repo
        .supersede_pending_pr_review_actions_except_head(&conversation_id, 267, "current-head")
        .await
        .unwrap();
    assert_eq!(superseded_ids, vec![stale.id.clone()]);

    let stale = repo
        .get_pr_review_action(&stale.id)
        .await
        .unwrap()
        .expect("stale action should exist");
    assert_eq!(stale.status, AgentWorkspacePrReviewActionStatus::Superseded);
    assert!(stale.resolved_at.is_some());
    let current = repo
        .get_pr_review_action(&current.id)
        .await
        .unwrap()
        .expect("current action should exist");
    assert_eq!(current.status, AgentWorkspacePrReviewActionStatus::Pending);
    let submitted = repo
        .get_pr_review_action(&submitted.id)
        .await
        .unwrap()
        .expect("submitted action should exist");
    assert_eq!(
        submitted.status,
        AgentWorkspacePrReviewActionStatus::Submitted
    );
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
async fn list_active_direct_published_workspaces_excludes_archived_conversation_owner() {
    let (db, repo, conversation_id) = setup_repo();
    let mut published = make_workspace(conversation_id.clone());
    published.publication_pr_number = Some(72);
    published.publication_pr_status = Some("open".to_string());
    published.publication_push_status = Some("pushed".to_string());
    repo.create_or_update(published).await.unwrap();

    db.with_connection(|conn| {
        conn.execute(
            "UPDATE chat_conversations
             SET archived_at = '2026-07-13T12:00:00Z'
             WHERE id = ?1",
            rusqlite::params![conversation_id.as_str()],
        )
        .unwrap();
    });

    let workspaces = repo
        .list_active_direct_published_workspaces()
        .await
        .unwrap();

    assert!(workspaces.is_empty());
}

#[tokio::test]
async fn list_active_pr_poller_recovery_workspaces_includes_supervised_ideation_and_review_prs() {
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

    let review_pr_id = ChatConversationId::from_string("30303030-3030-3030-3030-303030303030");
    seed_conversation(&db, &review_pr_id);
    let mut review_pr = make_workspace(review_pr_id);
    review_pr.mode = AgentConversationWorkspaceMode::ReviewPr;
    review_pr.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 75,
        url: Some("https://github.com/owner/repo/pull/75".to_string()),
        title: Some("Review PR source".to_string()),
        head_ref_name: "feature/review-pr".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head-75".to_string()),
    });
    review_pr.publication_pr_number = None;
    review_pr.publication_pr_status = None;
    review_pr.publication_push_status = None;
    repo.create_or_update(review_pr.clone()).await.unwrap();

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
        [
            direct.conversation_id,
            ideation.conversation_id,
            review_pr.conversation_id,
        ]
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

    let terminal_linked_id =
        ChatConversationId::from_string("26262626-2626-2626-2626-262626262626");
    seed_conversation(&db, &terminal_linked_id);
    let mut terminal_linked = make_workspace(terminal_linked_id);
    terminal_linked.publication_pr_number = Some(76);
    terminal_linked.publication_pr_status = Some("merged".to_string());
    terminal_linked.publication_push_status = Some("pushed".to_string());
    repo.create_or_update(terminal_linked.clone())
        .await
        .unwrap();

    let terminal_unlinked_id =
        ChatConversationId::from_string("27272727-2727-2727-2727-272727272727");
    seed_conversation(&db, &terminal_unlinked_id);
    let mut terminal_unlinked = make_workspace(terminal_unlinked_id);
    terminal_unlinked.publication_pr_status = Some("merged".to_string());
    repo.create_or_update(terminal_unlinked).await.unwrap();

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
            missing_linked.conversation_id,
            terminal_linked.conversation_id
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

    let handoff_id = ChatConversationId::from_string("89898989-8989-8989-8989-898989898989");
    seed_conversation(&db, &handoff_id);
    let mut handoff = make_workspace(handoff_id.clone());
    handoff.publication_pr_number = Some(87);
    handoff.publication_pr_status = Some("open".to_string());
    handoff.publication_push_status = Some("refreshed".to_string());
    handoff.pr_supervision_status = Some("reviewing".to_string());
    handoff.pr_autofix_enabled = true;
    repo.create_or_update(handoff).await.unwrap();

    let stranded_id = ChatConversationId::from_string("45454545-4545-4545-4545-454545454545");
    seed_conversation(&db, &stranded_id);
    let mut stranded = make_workspace(stranded_id.clone());
    stranded.publication_pr_number = Some(88);
    stranded.publication_pr_status = Some("open".to_string());
    stranded.publication_push_status = Some("refreshed".to_string());
    stranded.pr_supervision_status = Some("fixing".to_string());
    stranded.pr_autofix_enabled = true;
    repo.create_or_update(stranded).await.unwrap();

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

    assert_eq!(workspaces.len(), 3);
    assert!(workspaces
        .iter()
        .any(|workspace| workspace.conversation_id == candidate.conversation_id));
    assert!(workspaces
        .iter()
        .any(|workspace| workspace.conversation_id == handoff_id));
    assert!(workspaces
        .iter()
        .any(|workspace| workspace.conversation_id == stranded_id));

    let limited = repo
        .list_active_direct_pr_supervision_recovery_candidates(0)
        .await
        .unwrap();
    assert!(limited.is_empty());
}

#[tokio::test]
async fn linked_plan_pr_supervision_recovery_candidates_filter_ideation_rows() {
    let (db, repo, conversation_id) = setup_repo();
    let mut blocked = make_workspace(conversation_id);
    blocked.mode = AgentConversationWorkspaceMode::Ideation;
    blocked.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-linked-1"));
    blocked.pr_supervision_status = Some("blocked".to_string());
    blocked.pr_autofix_enabled = true;
    repo.create_or_update(blocked.clone()).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    let fixing_id = ChatConversationId::from_string("99999999-9999-9999-9999-999999999999");
    seed_conversation(&db, &fixing_id);
    let mut fixing = make_workspace(fixing_id);
    fixing.mode = AgentConversationWorkspaceMode::Ideation;
    fixing.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-linked-2"));
    fixing.pr_supervision_status = Some("fixing".to_string());
    fixing.pr_auto_merge_desired = true;
    repo.create_or_update(fixing.clone()).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    let direct_id = ChatConversationId::from_string("10101010-1010-1010-1010-101010101010");
    seed_conversation(&db, &direct_id);
    let mut direct = make_workspace(direct_id);
    direct.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-direct"));
    direct.pr_supervision_status = Some("blocked".to_string());
    direct.pr_autofix_enabled = true;
    repo.create_or_update(direct).await.unwrap();

    let unlinked_id = ChatConversationId::from_string("20202020-2020-2020-2020-202020202020");
    seed_conversation(&db, &unlinked_id);
    let mut unlinked = make_workspace(unlinked_id);
    unlinked.mode = AgentConversationWorkspaceMode::Ideation;
    unlinked.pr_supervision_status = Some("blocked".to_string());
    unlinked.pr_autofix_enabled = true;
    repo.create_or_update(unlinked).await.unwrap();

    let disabled_id = ChatConversationId::from_string("30303030-3030-3030-3030-303030303030");
    seed_conversation(&db, &disabled_id);
    let mut disabled = make_workspace(disabled_id);
    disabled.mode = AgentConversationWorkspaceMode::Ideation;
    disabled.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-disabled"));
    disabled.pr_supervision_status = Some("blocked".to_string());
    repo.create_or_update(disabled).await.unwrap();

    let monitoring_id = ChatConversationId::from_string("40404040-4040-4040-4040-404040404040");
    seed_conversation(&db, &monitoring_id);
    let mut monitoring = make_workspace(monitoring_id);
    monitoring.mode = AgentConversationWorkspaceMode::Ideation;
    monitoring.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-monitoring"));
    monitoring.pr_supervision_status = Some("monitoring".to_string());
    monitoring.pr_autofix_enabled = true;
    repo.create_or_update(monitoring).await.unwrap();

    let paused_id = ChatConversationId::from_string("50505050-5050-5050-5050-505050505050");
    seed_conversation(&db, &paused_id);
    let mut paused = make_workspace(paused_id);
    paused.mode = AgentConversationWorkspaceMode::Ideation;
    paused.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-paused"));
    paused.pr_supervision_status = Some("blocked".to_string());
    paused.pr_autofix_enabled = true;
    paused.auto_publish_enabled = false;
    repo.create_or_update(paused).await.unwrap();

    let limited = repo
        .list_active_linked_plan_pr_supervision_recovery_candidates(1)
        .await
        .unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].conversation_id, fixing.conversation_id);

    let workspaces = repo
        .list_active_linked_plan_pr_supervision_recovery_candidates(10)
        .await
        .unwrap();

    assert_eq!(
        workspaces
            .into_iter()
            .map(|workspace| workspace.conversation_id)
            .collect::<Vec<_>>(),
        vec![fixing.conversation_id, blocked.conversation_id]
    );
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
