use super::MemoryAgentConversationWorkspaceRepository;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    AgentWorkspacePrDescription, AgentWorkspacePrMetadataDecision, AgentWorkspacePrReviewAction,
    AgentWorkspacePrReviewActionKind, AgentWorkspacePrReviewActionStatus,
    AgentWorkspacePrReviewMonitor, AgentWorkspacePrReviewMonitorStatus,
    AgentWorkspaceRepairAttempt, AgentWorkspaceRepairContinuation, AgentWorkspaceRepairOutcome,
    AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource, AgentWorkspaceReviewApprovalSnapshot,
    AgentWorkspaceReviewAutoMergeGuard, AgentWorkspaceReviewAutoMergeGuardStatus,
    AgentWorkspaceReviewFixerSnapshot, AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor,
    AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, AgentWorkspaceSourcePullRequest, ArtifactId,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranchId, ProjectId,
    WORKSPACE_REVIEW_FIXER_STATUS_CYCLE_CAPPED, WORKSPACE_REVIEW_FIXER_STATUS_QUEUED,
    WORKSPACE_REVIEW_FIXER_STATUS_ROUTING, WORKSPACE_REVIEW_FIXER_STATUS_RUNNING,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceLocalCleanupClaim,
    AgentWorkspacePrReviewActionMutation, AgentWorkspacePublishLeaseClaim,
    AgentWorkspaceRepairRepository, AgentWorkspaceRepairStateGuard,
    AgentWorkspaceRepairStateTransition, SettleAndStartAgentWorkspaceRepairSuccessor,
    SettleAndStartAgentWorkspaceRepairSuccessorOutcome, StartOrJoinAgentWorkspaceRepairAttempt,
    StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};

#[tokio::test]
async fn publish_lease_claim_rejects_a_missing_workspace() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("19191919-1919-1919-1919-191919191919");

    let error = repo
        .claim_publish_lease(
            &conversation_id,
            "run-one",
            "token-one",
            chrono::Utc::now(),
            None,
            false,
        )
        .await
        .expect_err("missing workspace must fail closed");

    assert!(matches!(error, crate::error::AppError::NotFound(_)));
}

#[tokio::test]
async fn publish_lease_rejects_live_owner_and_fences_stale_token() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-publish-lease");
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .expect("workspace should persist");
    let now = chrono::Utc::now();

    assert_eq!(
        repo.claim_publish_lease(&conversation_id, "run-one", "token-one", now, None, false)
            .await
            .expect("first lease should claim"),
        AgentWorkspacePublishLeaseClaim::Claimed
    );
    assert_eq!(
        repo.claim_publish_lease(
            &conversation_id,
            "run-two",
            "token-two",
            now,
            Some("token-one"),
            false
        )
        .await
        .expect("live owner must hold lease"),
        AgentWorkspacePublishLeaseClaim::HeldByLiveOwner
    );
    assert!(!repo
        .release_publish_lease(&conversation_id, "stale-token", None, now)
        .await
        .expect("stale release is a clean rejection"));
    let held = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("read should succeed")
        .expect("workspace should remain");
    assert_eq!(held.publish_lease_token.as_deref(), Some("token-one"));
}

#[tokio::test]
async fn normal_workspace_upsert_preserves_publish_lease_authority() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("39393939-3939-4939-8939-393939393939");
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .expect("workspace should persist");
    let mut stale_workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace read should succeed")
        .expect("workspace should exist");
    let claimed_at = chrono::Utc::now();
    repo.claim_publish_lease(
        &conversation_id,
        "run-one",
        "token-one",
        claimed_at,
        None,
        false,
    )
    .await
    .expect("publish lease should claim");

    stale_workspace.base_commit = Some("updated-base".to_string());
    let updated = repo
        .create_or_update(stale_workspace)
        .await
        .expect("normal workspace fields should update");

    assert_eq!(updated.base_commit.as_deref(), Some("updated-base"));
    assert_eq!(
        updated.publish_lease_owner_run_id.as_deref(),
        Some("run-one")
    );
    assert_eq!(updated.publish_lease_token.as_deref(), Some("token-one"));
    assert_eq!(updated.publish_lease_heartbeat_at, Some(claimed_at));
    assert!(repo
        .release_publish_lease(
            &conversation_id,
            "token-one",
            Some("refreshed"),
            claimed_at + chrono::Duration::seconds(1),
        )
        .await
        .expect("the original owner should still release the lease"));
}

#[tokio::test]
async fn publish_lease_immediately_reclaims_a_dead_owner() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-publish-reclaim");
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .expect("workspace should persist");
    let now = chrono::Utc::now();
    repo.claim_publish_lease(&conversation_id, "dead-run", "dead-token", now, None, false)
        .await
        .expect("initial claim should succeed");

    assert_eq!(
        repo.claim_publish_lease(
            &conversation_id,
            "live-run",
            "fresh-token",
            now,
            Some("dead-token"),
            true,
        )
        .await
        .expect("dead owner should be reclaimed"),
        AgentWorkspacePublishLeaseClaim::Reclaimed
    );
    assert!(repo
        .heartbeat_publish_lease(&conversation_id, "fresh-token", now)
        .await
        .expect("current owner heartbeat should succeed"));

    assert_eq!(
        repo.claim_publish_lease(
            &conversation_id,
            "late-run",
            "late-token",
            now,
            Some("dead-token"),
            true,
        )
        .await
        .expect("stale reclaim proof should be rejected"),
        AgentWorkspacePublishLeaseClaim::HeldByLiveOwner
    );
}

fn pr_review_action(
    conversation_id: ChatConversationId,
    pr_number: i64,
    head_sha: &str,
) -> AgentWorkspacePrReviewAction {
    AgentWorkspacePrReviewAction::new(
        conversation_id,
        pr_number,
        head_sha.to_string(),
        AgentWorkspacePrReviewActionKind::RequestChanges,
        format!("Review {head_sha}"),
        format!("Body for {head_sha}"),
        None,
        Some(format!("run-{head_sha}")),
    )
}

#[tokio::test]
async fn pr_metadata_decisions_round_trip_legacy_and_clear() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-pr-metadata");
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

    repo.clear_pr_metadata_decision(&conversation_id)
        .await
        .unwrap();
    assert_eq!(
        repo.get_pr_metadata_decision(&conversation_id)
            .await
            .unwrap(),
        None
    );

    repo.save_pr_description(
        &conversation_id,
        AgentWorkspacePrDescription::new(
            Some("legacy title".to_string()),
            "legacy body".to_string(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        repo.get_pr_metadata_decision(&conversation_id)
            .await
            .unwrap(),
        Some(AgentWorkspacePrMetadataDecision::Patch {
            title: Some("legacy title".to_string()),
            body_markdown: Some("legacy body".to_string()),
        })
    );
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
async fn workspace_review_fixer_claim_is_exact_and_single_winner() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-fixer-claim");
    let artifact_id = ArtifactId::from_string("artifact-fixer-claim");
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id,
        ProjectId::from_string("project-memory".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("diff-claim".to_string());
    monitor.reviewed_diff_fingerprint = Some("diff-claim".to_string());
    monitor.current_plan_context_fingerprint = Some("plan-claim".to_string());
    monitor.reviewed_plan_context_fingerprint = Some("plan-claim".to_string());
    monitor.review_artifact_id = Some(artifact_id.clone());
    monitor.review_artifact_version = Some(4);
    monitor.review_requested_changes_artifact_id = Some(artifact_id.clone());
    monitor.review_requested_changes_artifact_version = Some(4);
    monitor.review_blocking_fingerprint = Some("blocker-claim".to_string());
    monitor.review_fixer_cycle_count = 2;
    repo.upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");
    let snapshot = AgentWorkspaceReviewFixerSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-claim".to_string(),
        requested_changes_artifact_id: artifact_id.clone(),
        requested_changes_artifact_version: 4,
        artifact_id,
        artifact_version: 4,
        blocking_fingerprint: "blocker-claim".to_string(),
        plan_context_fingerprint: Some("plan-claim".to_string()),
    };

    let mut stale_plan_snapshot = snapshot.clone();
    stale_plan_snapshot.plan_context_fingerprint = Some("plan-stale".to_string());
    assert!(repo
        .claim_workspace_review_fixer(
            &conversation_id,
            &stale_plan_snapshot,
            "attempt-stale-plan",
            chrono::Utc::now(),
        )
        .await
        .expect("stale plan claim should be a clean rejection")
        .is_none());
    let rejected = repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor read should succeed")
        .expect("rejected claim must not remove the monitor");
    assert_eq!(rejected.review_fixer_cycle_count, 2);
    assert!(rejected.review_fixer_attempt_id.is_none());

    let claimed = repo
        .claim_workspace_review_fixer(
            &conversation_id,
            &snapshot,
            "attempt-one",
            chrono::Utc::now(),
        )
        .await
        .expect("claim should succeed")
        .expect("exact snapshot should win");
    assert_eq!(claimed.review_fixer_status.as_deref(), Some("routing"));
    assert_eq!(
        claimed.review_fixer_attempt_id.as_deref(),
        Some("attempt-one")
    );
    assert_eq!(claimed.review_fixer_cycle_count, 3);
    assert!(repo
        .claim_workspace_review_fixer(
            &conversation_id,
            &snapshot,
            "attempt-two",
            chrono::Utc::now(),
        )
        .await
        .expect("losing claim should be a clean rejection")
        .is_none());
}

#[tokio::test]
async fn workspace_review_fixer_settlement_rejects_refreshed_target_authority() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-fixer-stale-settle");
    let artifact_id = ArtifactId::from_string("artifact-fixer-stale-settle");
    let snapshot = AgentWorkspaceReviewFixerSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-old".to_string(),
        artifact_id: artifact_id.clone(),
        artifact_version: 4,
        requested_changes_artifact_id: artifact_id.clone(),
        requested_changes_artifact_version: 4,
        blocking_fingerprint: "blocker-old".to_string(),
        plan_context_fingerprint: None,
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id,
        ProjectId::from_string("project-memory".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.current_target_scope = Some(snapshot.target_scope);
    monitor.reviewed_target_scope = Some(snapshot.target_scope);
    monitor.current_diff_fingerprint = Some(snapshot.diff_fingerprint.clone());
    monitor.reviewed_diff_fingerprint = Some(snapshot.diff_fingerprint.clone());
    monitor.current_plan_context_fingerprint = Some("plan-old".to_string());
    monitor.reviewed_plan_context_fingerprint = Some("plan-old".to_string());
    monitor.review_artifact_id = Some(artifact_id);
    monitor.review_artifact_version = Some(snapshot.artifact_version);
    monitor.review_requested_changes_artifact_id =
        Some(snapshot.requested_changes_artifact_id.clone());
    monitor.review_requested_changes_artifact_version =
        Some(snapshot.requested_changes_artifact_version);
    monitor.review_blocking_fingerprint = Some(snapshot.blocking_fingerprint.clone());
    let snapshot = AgentWorkspaceReviewFixerSnapshot {
        plan_context_fingerprint: Some("plan-old".to_string()),
        ..snapshot
    };
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
async fn workspace_review_fixer_settlement_rejects_refreshed_plan_authority() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-fixer-plan-settle");
    let artifact_id = ArtifactId::from_string("artifact-fixer-plan-settle");
    let snapshot = AgentWorkspaceReviewFixerSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-current".to_string(),
        artifact_id: artifact_id.clone(),
        artifact_version: 4,
        requested_changes_artifact_id: artifact_id.clone(),
        requested_changes_artifact_version: 4,
        blocking_fingerprint: "blocker-current".to_string(),
        plan_context_fingerprint: Some("plan-reviewed".to_string()),
    };
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id,
        ProjectId::from_string("project-memory".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.current_target_scope = Some(snapshot.target_scope);
    monitor.reviewed_target_scope = Some(snapshot.target_scope);
    monitor.current_diff_fingerprint = Some(snapshot.diff_fingerprint.clone());
    monitor.reviewed_diff_fingerprint = Some(snapshot.diff_fingerprint.clone());
    monitor.current_plan_context_fingerprint = snapshot.plan_context_fingerprint.clone();
    monitor.reviewed_plan_context_fingerprint = snapshot.plan_context_fingerprint.clone();
    monitor.review_artifact_id = Some(artifact_id);
    monitor.review_artifact_version = Some(snapshot.artifact_version);
    monitor.review_requested_changes_artifact_id =
        Some(snapshot.requested_changes_artifact_id.clone());
    monitor.review_requested_changes_artifact_version =
        Some(snapshot.requested_changes_artifact_version);
    monitor.review_blocking_fingerprint = Some(snapshot.blocking_fingerprint.clone());
    repo.upsert_workspace_review_monitor(monitor).await.unwrap();
    let mut claimed = repo
        .claim_workspace_review_fixer(
            &conversation_id,
            &snapshot,
            "attempt-plan-stale",
            chrono::Utc::now(),
        )
        .await
        .unwrap()
        .unwrap();

    let mut refreshed = claimed.clone();
    refreshed.current_plan_context_fingerprint = Some("plan-new".to_string());
    repo.upsert_workspace_review_monitor(refreshed.clone())
        .await
        .unwrap();
    claimed.review_fixer_status = Some("running".to_string());

    assert!(repo
        .settle_workspace_review_fixer_attempt(claimed, "attempt-plan-stale", &snapshot)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        repo.get_workspace_review_monitor(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .current_plan_context_fingerprint,
        refreshed.current_plan_context_fingerprint
    );
}

#[tokio::test]
async fn invalid_workspace_review_fixer_attempt_failure_is_attempt_scoped() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-invalid-fixer");
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id,
        ProjectId::from_string("project-memory".to_string()),
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

#[tokio::test]
async fn latest_pending_pr_review_action_is_deterministic_and_owner_scoped() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
    let other_conversation_id =
        ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");

    let terminal = repo
        .create_or_update_pr_review_action(pr_review_action(
            conversation_id.clone(),
            411,
            "terminal-head",
        ))
        .await
        .expect("create terminal action");
    repo.update_pr_review_action_status(
        &terminal.id,
        AgentWorkspacePrReviewActionStatus::Submitted,
        Some("review-terminal"),
    )
    .await
    .expect("resolve terminal action");

    let mut older_input = pr_review_action(conversation_id.clone(), 411, "older-head");
    older_input.id = "tie-action-a".to_string();
    let older = repo
        .create_or_update_pr_review_action(older_input)
        .await
        .expect("create older pending action");
    let mut latest_input = pr_review_action(conversation_id.clone(), 411, "latest-head");
    latest_input.id = "tie-action-b".to_string();
    let latest = repo
        .create_or_update_pr_review_action(latest_input)
        .await
        .expect("create latest pending action");
    repo.create_or_update_pr_review_action(pr_review_action(
        conversation_id.clone(),
        412,
        "other-pr-head",
    ))
    .await
    .expect("create other PR action");
    repo.create_or_update_pr_review_action(pr_review_action(
        other_conversation_id.clone(),
        411,
        "other-conversation-head",
    ))
    .await
    .expect("create other conversation action");

    let tied_at = chrono::DateTime::parse_from_rfc3339("2026-07-20T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let mut actions = repo.pr_review_actions.write().await;
    for id in [&older.id, &latest.id] {
        let action = actions.get_mut(id).expect("seeded tie action");
        action.created_at = tied_at;
        action.updated_at = tied_at;
    }
    drop(actions);

    let selected = repo
        .get_latest_pending_pr_review_action(&conversation_id, 411)
        .await
        .expect("read latest pending action")
        .expect("latest pending action exists");

    let expected_tie_winner = std::cmp::max(older.id.clone(), latest.id.clone());
    assert_eq!(selected.id, expected_tie_winner);
    assert_ne!(selected.id, terminal.id);
    assert!(repo
        .get_latest_pending_pr_review_action(&other_conversation_id, 412)
        .await
        .expect("read isolated owner")
        .is_none());
}

#[tokio::test]
async fn terminal_settlement_supersedes_pending_and_submitting_actions_once() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("review-terminal-memory");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 411,
        url: None,
        title: None,
        head_ref_name: "feature/review".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head".to_string()),
    });
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_supervision_summary = Some("Waiting".to_string());
    repo.create_or_update(workspace).await.unwrap();

    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-memory".to_string()),
        411,
        Some("head".to_string()),
    );
    monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
    repo.upsert_pr_review_monitor(monitor).await.unwrap();
    let pending = repo
        .create_or_update_pr_review_action(pr_review_action(conversation_id.clone(), 411, "head"))
        .await
        .unwrap();
    let submitting = repo
        .create_or_update_pr_review_action(pr_review_action(
            conversation_id.clone(),
            411,
            "older-head",
        ))
        .await
        .unwrap();
    assert!(repo
        .claim_pending_pr_review_action(&submitting.id)
        .await
        .unwrap());

    let first = repo
        .settle_pr_review_terminal(&conversation_id, 411, "closed", "Closed without merge")
        .await
        .unwrap();
    assert!(first.event_inserted);
    assert_eq!(first.superseded_action_ids.len(), 2);
    let second = repo
        .settle_pr_review_terminal(&conversation_id, 411, "closed", "Closed without merge")
        .await
        .unwrap();
    assert!(!second.event_inserted);
    assert_eq!(second.superseded_action_ids.len(), 2);
    for id in [pending.id, submitting.id] {
        assert_eq!(
            repo.get_pr_review_action(&id)
                .await
                .unwrap()
                .unwrap()
                .status,
            AgentWorkspacePrReviewActionStatus::Superseded
        );
    }
    let workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.publication_pr_status.as_deref(), Some("closed"));
    assert!(workspace.pr_supervision_status.is_none());
    let monitor = repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        monitor.status,
        AgentWorkspacePrReviewMonitorStatus::Terminal
    );
    assert_eq!(monitor.last_error.as_deref(), Some("Closed without merge"));
}

#[tokio::test]
async fn lifecycle_listing_keeps_paused_review_pr_monitor_visible() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("paused-review-lifecycle");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 412,
        url: None,
        title: None,
        head_ref_name: "feature/paused".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head".to_string()),
    });
    repo.create_or_update(workspace).await.unwrap();
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id,
        ProjectId::from_string("project-memory".to_string()),
        412,
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
async fn guarded_pr_review_transition_settles_action_and_monitor_without_terminal_resurrection() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("guarded-review-transition-memory");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 413,
        url: None,
        title: None,
        head_ref_name: "feature/guarded".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head".to_string()),
    });
    repo.create_or_update(workspace).await.unwrap();
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-memory".to_string()),
        413,
        Some("head".to_string()),
    );
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    monitor = repo.upsert_pr_review_monitor(monitor).await.unwrap();

    let action = pr_review_action(conversation_id.clone(), 413, "head");
    monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
    let proposed = repo
        .transition_pr_review_state_if_nonterminal(
            monitor.clone(),
            Some(AgentWorkspacePrReviewActionMutation::UpsertPending(
                action.clone(),
            )),
        )
        .await
        .unwrap()
        .expect("proposal transition should commit");
    assert_eq!(
        proposed.monitor.status,
        AgentWorkspacePrReviewMonitorStatus::AwaitingUser
    );
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
    repo.settle_pr_review_terminal(&conversation_id, 413, "merged", "Merged")
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
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("terminal-rearm-memory");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 414,
        url: None,
        title: None,
        head_ref_name: "feature/rearm".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head".to_string()),
    });
    repo.create_or_update(workspace).await.unwrap();

    assert!(repo
        .settle_pr_review_terminal(&conversation_id, 414, "invalid", "Invalid")
        .await
        .is_err());
    let settled = repo
        .settle_pr_review_terminal(&conversation_id, 414, "merged", "Merged")
        .await
        .unwrap();
    assert!(settled.superseded_action_ids.is_empty());
    assert!(repo
        .rearm_terminal_pr_review_monitor_after_live_open(&conversation_id, 414)
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
        Some(414),
        None,
        Some("open"),
        Some("pushed"),
    )
    .await
    .unwrap();
    let rearmed = repo
        .rearm_terminal_pr_review_monitor_after_live_open(&conversation_id, 414)
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
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("guarded-actions-memory");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 415,
        url: None,
        title: None,
        head_ref_name: "feature/guarded-actions".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head".to_string()),
    });
    repo.create_or_update(workspace).await.unwrap();

    let action = pr_review_action(conversation_id.clone(), 415, "head");
    let saved = repo
        .create_or_update_pr_review_action_if_nonterminal(action.clone())
        .await
        .unwrap();
    assert!(!repo
        .claim_pending_pr_review_action_if_nonterminal(&saved.id, &conversation_id, 999)
        .await
        .unwrap());
    assert!(repo
        .claim_pending_pr_review_action_if_nonterminal(&saved.id, &conversation_id, 415)
        .await
        .unwrap());

    repo.settle_pr_review_terminal(&conversation_id, 415, "closed", "Closed")
        .await
        .unwrap();
    assert!(repo
        .create_or_update_pr_review_action_if_nonterminal(pr_review_action(
            conversation_id.clone(),
            415,
            "new-head",
        ))
        .await
        .is_err());
    assert!(!repo
        .claim_pending_pr_review_action_if_nonterminal(&saved.id, &conversation_id, 415)
        .await
        .unwrap());
}

fn make_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("project-memory".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project-memory/agent".to_string(),
        "/tmp/ralphx/project-memory/agent".to_string(),
    )
}

#[tokio::test]
async fn review_automation_override_resets_budget_and_preserves_active_attempt_identity() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("review-automation-memory");
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .expect("workspace should persist");
    let mut capped = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-memory".to_string()),
    );
    capped.review_fixer_cycle_count = 3;
    capped.review_fixer_status = Some(WORKSPACE_REVIEW_FIXER_STATUS_CYCLE_CAPPED.to_string());
    capped.review_fixer_attempt_id = Some("capped-attempt".to_string());
    repo.upsert_workspace_review_monitor(capped)
        .await
        .expect("capped monitor should persist");

    repo.set_review_automation_override(&conversation_id, Some(true))
        .await
        .expect("rearm should persist");
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace should load")
            .expect("workspace should exist")
            .review_automation_override,
        Some(true)
    );
    let rearmed = repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor should load")
        .expect("monitor should exist");
    assert_eq!(rearmed.review_fixer_cycle_count, 0);
    assert!(rearmed.review_fixer_status.is_none());
    assert!(rearmed.review_fixer_attempt_id.is_none());

    let mut idle = rearmed;
    idle.review_fixer_cycle_count = 2;
    repo.upsert_workspace_review_monitor(idle)
        .await
        .expect("idle monitor should persist");
    repo.set_review_automation_override(&conversation_id, Some(false))
        .await
        .expect("disarm should persist without resetting the budget");
    let idle = repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("idle monitor should load")
        .expect("idle monitor should exist");
    assert_eq!(idle.review_fixer_cycle_count, 2);

    repo.set_review_automation_override(&conversation_id, Some(true))
        .await
        .expect("arming should reset an idle budget");
    let mut failed = repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("rearmed idle monitor should load")
        .expect("rearmed idle monitor should exist");
    assert_eq!(failed.review_fixer_cycle_count, 0);
    failed.review_fixer_cycle_count = 2;
    failed.review_fixer_status = Some("failed".to_string());
    failed.review_fixer_attempt_id = Some("failed-attempt".to_string());
    repo.upsert_workspace_review_monitor(failed)
        .await
        .expect("failed monitor should persist");
    repo.set_review_automation_override(&conversation_id, Some(true))
        .await
        .expect("arming should reset a settled failed budget");
    let failed = repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("failed monitor should load")
        .expect("failed monitor should exist");
    assert_eq!(failed.review_fixer_cycle_count, 0);
    assert_eq!(failed.review_fixer_status.as_deref(), Some("failed"));
    assert_eq!(
        failed.review_fixer_attempt_id.as_deref(),
        Some("failed-attempt")
    );

    for status in [
        WORKSPACE_REVIEW_FIXER_STATUS_ROUTING,
        WORKSPACE_REVIEW_FIXER_STATUS_QUEUED,
        WORKSPACE_REVIEW_FIXER_STATUS_RUNNING,
    ] {
        let mut active = failed.clone();
        active.review_fixer_cycle_count = 2;
        active.review_fixer_status = Some(status.to_string());
        active.review_fixer_attempt_id = Some(format!("{status}-attempt"));
        repo.upsert_workspace_review_monitor(active)
            .await
            .expect("active monitor should persist");
        repo.set_review_automation_override(&conversation_id, Some(true))
            .await
            .expect("active automation preference should persist");
        let active = repo
            .get_workspace_review_monitor(&conversation_id)
            .await
            .expect("active monitor should load")
            .expect("active monitor should exist");
        assert_eq!(active.review_fixer_cycle_count, 0);
        assert_eq!(active.review_fixer_status.as_deref(), Some(status));
        assert_eq!(
            active.review_fixer_attempt_id.as_deref(),
            Some(format!("{status}-attempt").as_str())
        );
    }
}

#[tokio::test]
async fn repair_state_cas_is_atomic_and_rejects_a_stale_guard() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("repair-state-memory");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_supervision_summary = Some("old blocker".to_string());
    workspace.pr_supervision_updated_at = None;
    workspace.pr_auto_merge_current = Some(true);
    workspace.publication_pr_url = Some("https://example.test/pr/7".to_string());
    repo.create_or_update(workspace.clone()).await.unwrap();

    let claimed_at = chrono::Utc::now();
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("needs_agent".to_string()),
        pr_supervision_status: Some("fixing".to_string()),
        pr_supervision_summary: Some("Repair is running.".to_string()),
        pr_supervision_updated_at: claimed_at,
        pr_auto_merge_current: Some(false),
        base_commit: Some("base-repaired".to_string()),
    };
    assert!(repo
        .compare_and_set_repair_state(
            &conversation_id,
            &AgentWorkspaceRepairStateGuard::from_workspace(&workspace),
            &transition,
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

    let mut stale = AgentWorkspaceRepairStateGuard::from_workspace(&claimed);
    stale.pr_supervision_updated_at = None;
    let blocked_at = claimed_at + chrono::Duration::seconds(1);
    assert!(!repo
        .compare_and_set_repair_state(
            &conversation_id,
            &stale,
            &AgentWorkspaceRepairStateTransition {
                publication_push_status: Some("needs_agent".to_string()),
                pr_supervision_status: Some("blocked".to_string()),
                pr_supervision_summary: Some("stale failure".to_string()),
                pr_supervision_updated_at: blocked_at,
                pr_auto_merge_current: None,
                base_commit: None,
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
async fn repair_state_and_events_are_all_or_nothing_in_memory() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("repair-state-events-memory");
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

    repo.fail_next_matching_publication_event(
        "pr_autofix_workspace_review_aborted",
        "failed",
        "injected event failure",
    );
    assert!(repo
        .compare_and_set_repair_state_with_events(
            &conversation_id,
            &AgentWorkspaceRepairStateGuard::from_workspace(&workspace),
            &transition,
            vec![event.clone()],
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

    let mut stale = AgentWorkspaceRepairStateGuard::from_workspace(&workspace);
    stale.pr_supervision_updated_at = None;
    assert!(!repo
        .compare_and_set_repair_state_with_events(
            &conversation_id,
            &stale,
            &transition,
            vec![event],
        )
        .await
        .unwrap());
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn legacy_repair_cas_cannot_mutate_a_durable_generation() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("repair-state-durable-memory");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_supervision_updated_at = Some(chrono::Utc::now());
    repo.create_or_update(workspace.clone()).await.unwrap();

    let attempt = AgentWorkspaceRepairAttempt::new(
        conversation_id.clone(),
        AgentWorkspaceRepairSource::Publish,
        AgentWorkspaceRepairContinuation::Publish,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    let durable = match repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt,
            reason: "durable owner".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .unwrap()
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected durable attempt, got {outcome:?}"),
    };
    assert_eq!(durable.phase, AgentWorkspaceRepairPhase::Requested);
    let before = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("pushed".to_string()),
        pr_supervision_status: Some("publishing".to_string()),
        pr_supervision_summary: Some("stale legacy success".to_string()),
        pr_supervision_updated_at: before.pr_supervision_updated_at.unwrap()
            + chrono::Duration::seconds(1),
        pr_auto_merge_current: Some(true),
        base_commit: Some("stale-base".to_string()),
    };
    let guard = AgentWorkspaceRepairStateGuard::from_workspace(&before);
    assert!(!repo
        .compare_and_set_repair_state(&conversation_id, &guard, &transition)
        .await
        .unwrap());
    assert!(!repo
        .compare_and_set_repair_state_with_events(
            &conversation_id,
            &guard,
            &transition,
            vec![AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "legacy_repair_succeeded",
                "succeeded",
                "stale legacy success",
                Some("legacy".to_string()),
            )],
        )
        .await
        .unwrap());
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap(),
        before
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        repo.get_current_repair_attempt(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .id,
        durable.id
    );
}

#[tokio::test]
async fn repair_attempts_list_is_generation_ordered_and_conversation_scoped() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    // `ChatConversationId::from_string` collapses non-UUID text to the nil UUID, so scoping can
    // only be proven with two genuinely distinct ids.
    let conversation_id = ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
    let other_conversation_id =
        ChatConversationId::from_string("44444444-4444-4444-4444-444444444444");
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .unwrap();
    repo.create_or_update(make_workspace(other_conversation_id.clone()))
        .await
        .unwrap();

    // The in-memory repo backs a hash map, so ordering only holds because the listing sorts by
    // generation. Fingerprint spend reads the whole history, so both ordering and scoping matter.
    let first = start_memory_repair_attempt(&repo, &conversation_id, "first generation").await;
    let second = match repo
        .settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
            attempt_id: first.id.clone(),
            generation: first.generation,
            expected_phase: first.phase,
            expected_updated_at: first.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Superseded,
            settled_at: chrono::Utc::now(),
            successor: StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: AgentWorkspaceRepairAttempt::new(
                    conversation_id.clone(),
                    AgentWorkspaceRepairSource::Publish,
                    AgentWorkspaceRepairContinuation::Publish,
                    "main",
                    false,
                    true,
                    false,
                    None,
                    chrono::Utc::now(),
                ),
                reason: "second generation".to_string(),
                verified_newer_base: false,
                compatibility_projection: None,
                events: Vec::new(),
            },
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .unwrap()
    {
        SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Started(attempt) => attempt,
        outcome => panic!("expected a started successor, got {outcome:?}"),
    };
    let foreign =
        start_memory_repair_attempt(&repo, &other_conversation_id, "unrelated workspace").await;

    assert_eq!(
        repo.list_repair_attempts_for_conversation(&conversation_id)
            .await
            .unwrap()
            .into_iter()
            .map(|attempt| (attempt.id, attempt.generation))
            .collect::<Vec<_>>(),
        vec![(first.id, 1), (second.id, 2)]
    );
    assert_eq!(
        repo.list_repair_attempts_for_conversation(&other_conversation_id)
            .await
            .unwrap()
            .into_iter()
            .map(|attempt| attempt.id)
            .collect::<Vec<_>>(),
        vec![foreign.id]
    );
}

async fn start_memory_repair_attempt(
    repo: &MemoryAgentConversationWorkspaceRepository,
    conversation_id: &ChatConversationId,
    reason: &str,
) -> AgentWorkspaceRepairAttempt {
    match repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                conversation_id.clone(),
                AgentWorkspaceRepairSource::Publish,
                AgentWorkspaceRepairContinuation::Publish,
                "main",
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: reason.to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .unwrap()
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected a started attempt, got {outcome:?}"),
    }
}

#[tokio::test]
async fn pr_poller_recovery_includes_review_pr_without_owned_automation_flags() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("review-pr-recovery-no-automation");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 779,
        url: Some("https://github.com/owner/repo/pull/779".to_string()),
        title: Some("External review target".to_string()),
        head_ref_name: "external/head".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("review-head".to_string()),
    });
    workspace.pr_autofix_enabled = false;
    workspace.pr_auto_merge_desired = false;
    workspace.publication_push_status = None;
    workspace.auto_publish_enabled = true;
    repo.create_or_update(workspace)
        .await
        .expect("Review PR workspace should persist");

    let recovered = repo
        .list_active_pr_poller_recovery_workspaces()
        .await
        .expect("recovery query should succeed");

    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].conversation_id, conversation_id);
    assert!(!recovered[0].pr_autofix_enabled);
    assert!(!recovered[0].pr_auto_merge_desired);
}

#[tokio::test]
async fn restart_restore_reactivates_workspace_and_clears_cleanup_marker() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-restart");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.status = AgentConversationWorkspaceStatus::Missing;
    repo.create_or_update(workspace)
        .await
        .expect("insert missing workspace");
    repo.mark_local_cleanup_status(&conversation_id, "cleaned", chrono::Utc::now())
        .await
        .expect("mark cleanup");
    let session_id = IdeationSessionId::from_string("session-after-restart");
    let plan_branch_id = PlanBranchId::from_string("plan-branch-after-restart");

    repo.restore_after_restart(&conversation_id, &session_id, &plan_branch_id)
        .await
        .expect("restore after restart");

    let restored = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("read workspace")
        .expect("workspace remains persisted");
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
            .expect("read cleanup marker"),
        None
    );
}

#[tokio::test]
async fn restart_restore_rejects_missing_workspace() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let error = repo
        .restore_after_restart(
            &ChatConversationId::from_string("missing-conversation"),
            &IdeationSessionId::from_string("session-after-restart"),
            &PlanBranchId::from_string("plan-branch-after-restart"),
        )
        .await
        .expect_err("restore should require an existing workspace");

    assert!(error.to_string().contains("Workspace not found"));
}

#[tokio::test]
async fn approve_workspace_review_anyway_is_exact_and_single_use() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-review-bypass");
    repo.create_or_update(make_workspace(conversation_id))
        .await
        .expect("insert workspace");
    let artifact_id = ArtifactId::from_string("artifact-review-bypass");
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id,
        ProjectId::from_string("project-memory".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("diff-1".to_string());
    monitor.reviewed_diff_fingerprint = Some("diff-1".to_string());
    monitor.review_artifact_id = Some(artifact_id.clone());
    monitor.review_artifact_version = Some(2);
    monitor.review_requested_changes_artifact_id =
        Some(ArtifactId::from_string("changes-review-bypass"));
    monitor.review_requested_changes_artifact_version = Some(2);
    repo.upsert_workspace_review_monitor(monitor)
        .await
        .expect("insert blocking monitor");
    let snapshot = AgentWorkspaceReviewApprovalSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-1".to_string(),
        artifact_id,
        artifact_version: 2,
    };

    let applied = repo
        .approve_workspace_review_anyway(&conversation_id, &snapshot, chrono::Utc::now())
        .await
        .expect("approve exact snapshot")
        .expect("transition should apply");
    assert_eq!(
        applied.review_outcome,
        AgentWorkspaceReviewOutcome::Blocking
    );
    assert_eq!(
        applied.review_gate_status,
        AgentWorkspaceReviewGateStatus::Passed
    );

    assert!(repo
        .approve_workspace_review_anyway(&conversation_id, &snapshot, chrono::Utc::now())
        .await
        .expect("retry should be a no-op")
        .is_none());
    let events = repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list audit events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.step == "workspace_review_approved_anyway")
            .count(),
        1
    );
}

#[tokio::test]
async fn approve_workspace_review_anyway_rejects_active_publish_without_audit() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-review-bypass-publishing");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.publication_push_status = Some("checking".to_string());
    repo.create_or_update(workspace)
        .await
        .expect("insert publishing workspace");
    let artifact_id = ArtifactId::from_string("artifact-review-bypass-publishing");
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-memory".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("diff-publishing".to_string());
    monitor.reviewed_diff_fingerprint = Some("diff-publishing".to_string());
    monitor.review_artifact_id = Some(artifact_id.clone());
    monitor.review_artifact_version = Some(7);
    repo.upsert_workspace_review_monitor(monitor)
        .await
        .expect("insert blocking monitor");
    let snapshot = AgentWorkspaceReviewApprovalSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "diff-publishing".to_string(),
        artifact_id,
        artifact_version: 7,
    };

    assert!(repo
        .approve_workspace_review_anyway(&conversation_id, &snapshot, chrono::Utc::now())
        .await
        .expect("approval check should not fail")
        .is_none());
    let stored = repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("load monitor")
        .expect("monitor remains");
    assert_eq!(
        stored.review_gate_status,
        AgentWorkspaceReviewGateStatus::Blocking
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list audit events")
        .is_empty());
}

#[tokio::test]
async fn reserved_workspace_review_start_failure_is_exact_and_cannot_clobber_newer_run() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-reserved-review");
    let review_conversation_id = ChatConversationId::from_string("review-conversation-new");
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-memory".to_string()),
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

#[tokio::test]
async fn cleanup_status_round_trips_and_clears() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-cleanup");

    repo.mark_local_cleanup_status(&conversation_id, "unsafe", chrono::Utc::now())
        .await
        .expect("mark cleanup");
    assert_eq!(
        repo.get_local_cleanup_status(&conversation_id)
            .await
            .expect("read marker")
            .as_deref(),
        Some("unsafe")
    );

    repo.clear_local_cleanup_status(&conversation_id)
        .await
        .expect("clear marker");

    assert_eq!(
        repo.get_local_cleanup_status(&conversation_id)
            .await
            .expect("read cleared marker"),
        None
    );
}

#[tokio::test]
async fn local_cleanup_claim_is_single_flight_and_cleaned_is_monotonic() {
    let repo = std::sync::Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let conversation_id = ChatConversationId::from_string("conversation-cleanup-claim");
    repo.create_or_update(make_workspace(conversation_id.clone()))
        .await
        .expect("insert workspace");
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
            "failed_operational",
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
async fn terminal_cleanup_candidates_include_only_stale_retryable_markers() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let project_id = ProjectId::from_string("project-memory".to_string());
    let stale_checked_at = chrono::Utc::now() - chrono::Duration::days(30);
    let fresh_checked_at = chrono::Utc::now();
    let retryable_statuses = [
        "pending",
        "failed",
        "failed_unsafe",
        "failed_operational",
        "unsafe",
        "target_ref_missing",
        "workspace_dirty",
        "branch_missing",
        "cleaning",
    ];

    let mut retryable_conversation_ids = Vec::new();
    for status in retryable_statuses {
        let conversation_id = ChatConversationId::new();
        let mut workspace = make_workspace(conversation_id.clone());
        workspace.status = AgentConversationWorkspaceStatus::Active;
        workspace.publication_pr_status = Some("merged".to_string());
        repo.create_or_update(workspace)
            .await
            .expect("insert terminal workspace");
        repo.mark_local_cleanup_status(&conversation_id, status, stale_checked_at)
            .await
            .expect("mark stale retryable cleanup");
        retryable_conversation_ids.push((status, conversation_id));
    }
    let fresh_id = ChatConversationId::new();
    let mut fresh_workspace = make_workspace(fresh_id.clone());
    fresh_workspace.status = AgentConversationWorkspaceStatus::Active;
    fresh_workspace.publication_pr_status = Some("closed".to_string());
    repo.create_or_update(fresh_workspace)
        .await
        .expect("insert fresh terminal workspace");
    repo.mark_local_cleanup_status(&fresh_id, "cleaning", fresh_checked_at)
        .await
        .expect("mark fresh cleanup");
    let non_terminal_id = ChatConversationId::new();
    repo.create_or_update(make_workspace(non_terminal_id))
        .await
        .expect("insert active workspace");

    let candidates = repo
        .get_terminal_local_cleanup_candidates_by_project_id(&project_id)
        .await
        .expect("list terminal cleanup candidates");

    assert_eq!(candidates.len(), retryable_statuses.len());
    for (status, conversation_id) in retryable_conversation_ids {
        assert!(
            candidates
                .iter()
                .any(|workspace| workspace.conversation_id == conversation_id),
            "stale retryable marker {status} should be returned"
        );
    }
    assert!(!candidates
        .iter()
        .any(|workspace| workspace.conversation_id == fresh_id));
}

#[tokio::test]
async fn pr_review_auto_approve_settings_and_claim_round_trip() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let project_id = ProjectId::from_string("project-1".to_string());

    repo.upsert_pr_review_monitor(AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        project_id.clone(),
        702,
        Some("head-a".to_string()),
    ))
    .await
    .expect("insert monitor");

    let updated = repo
        .set_pr_review_auto_approve_enabled(&conversation_id, false)
        .await
        .expect("disable auto approve");
    assert!(!updated.auto_approve_enabled);
    assert!(!updated.first_action_resolved);

    let resolved = repo
        .mark_pr_review_first_action_resolved(&conversation_id)
        .await
        .expect("mark first action resolved");
    assert!(resolved.first_action_resolved);

    repo.upsert_pr_review_monitor(AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        project_id,
        702,
        Some("head-b".to_string()),
    ))
    .await
    .expect("upsert monitor preserves auto approve preferences");

    let preserved = repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .expect("load monitor")
        .expect("monitor exists");
    assert!(!preserved.auto_approve_enabled);
    assert!(preserved.first_action_resolved);
    assert_eq!(preserved.last_seen_head_sha.as_deref(), Some("head-b"));

    let action = AgentWorkspacePrReviewAction::new(
        conversation_id,
        702,
        "head-b".to_string(),
        AgentWorkspacePrReviewActionKind::Approve,
        "passes".to_string(),
        "approved".to_string(),
        None,
        Some("review-run-1".to_string()),
    );
    let action_id = action.id.clone();
    repo.create_or_update_pr_review_action(action)
        .await
        .expect("insert action");

    assert!(repo
        .claim_pending_pr_review_action(&action_id)
        .await
        .expect("claim pending action"));
    assert!(!repo
        .claim_pending_pr_review_action(&action_id)
        .await
        .expect("do not claim non-pending action"));
    assert!(!repo
        .claim_pending_pr_review_action("missing-action")
        .await
        .expect("missing action is not claimed"));

    let claimed = repo
        .get_pr_review_action(&action_id)
        .await
        .expect("load action")
        .expect("action exists");
    assert_eq!(
        claimed.status,
        AgentWorkspacePrReviewActionStatus::Submitting
    );
}

#[tokio::test]
async fn pr_review_monitor_rejects_stale_disabled_upserts_after_pause_and_restart() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-paused");
    let project_id = ProjectId::from_string("project-paused".to_string());
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        project_id,
        703,
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
        .expect("insert monitor");

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
        .expect("pause monitor");

    let stale_write = repo
        .upsert_pr_review_monitor(stale_disabled_callback.clone())
        .await
        .expect("stale callback write");
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
        .expect("explicit restart");
    assert!(restarted.monitor_enabled);
    assert_eq!(
        restarted.status,
        AgentWorkspacePrReviewMonitorStatus::Watching
    );

    let stale_after_restart = repo
        .upsert_pr_review_monitor(stale_disabled_callback)
        .await
        .expect("stale callback after restart");
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
async fn supersede_pending_pr_review_actions_except_head_keeps_current_and_terminal_actions() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-actions");
    let stale = repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            703,
            "old-head".to_string(),
            AgentWorkspacePrReviewActionKind::RequestChanges,
            "Old blocking issues".to_string(),
            "Please address old issues.".to_string(),
            None,
            Some("run-old".to_string()),
        ))
        .await
        .expect("insert stale action");
    let current = repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            703,
            "current-head".to_string(),
            AgentWorkspacePrReviewActionKind::Approve,
            "Current head passes".to_string(),
            "Approved.".to_string(),
            None,
            Some("run-current".to_string()),
        ))
        .await
        .expect("insert current action");
    let submitted = repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            703,
            "submitted-head".to_string(),
            AgentWorkspacePrReviewActionKind::RequestChanges,
            "Already submitted".to_string(),
            "Submitted.".to_string(),
            None,
            Some("run-submitted".to_string()),
        ))
        .await
        .expect("insert submitted action");
    repo.update_pr_review_action_status(
        &submitted.id,
        AgentWorkspacePrReviewActionStatus::Submitted,
        Some("review-submitted"),
    )
    .await
    .expect("mark submitted");

    let superseded_ids = repo
        .supersede_pending_pr_review_actions_except_head(&conversation_id, 703, "current-head")
        .await
        .expect("supersede old pending actions");
    assert_eq!(superseded_ids, vec![stale.id.clone()]);

    let stale = repo
        .get_pr_review_action(&stale.id)
        .await
        .expect("load stale action")
        .expect("stale action should exist");
    assert_eq!(stale.status, AgentWorkspacePrReviewActionStatus::Superseded);
    assert!(stale.resolved_at.is_some());
    let current = repo
        .get_pr_review_action(&current.id)
        .await
        .expect("load current action")
        .expect("current action should exist");
    assert_eq!(current.status, AgentWorkspacePrReviewActionStatus::Pending);
    let submitted = repo
        .get_pr_review_action(&submitted.id)
        .await
        .expect("load submitted action")
        .expect("submitted action should exist");
    assert_eq!(
        submitted.status,
        AgentWorkspacePrReviewActionStatus::Submitted
    );
}

#[tokio::test]
async fn workspace_review_auto_merge_guard_survives_monitor_updates_and_requires_its_owner() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("workspace-review-guard");
    let project_id = ProjectId::from_string("project-1".to_string());
    let guard = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "workspace-delta".to_string(),
        head_sha: Some("head-sha".to_string()),
        last_error: None,
    };
    let mut guarded = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id.clone());
    guarded.auto_merge_guard = Some(guard.clone());
    repo.upsert_workspace_review_monitor(guarded)
        .await
        .expect("guarded monitor should persist");

    repo.upsert_workspace_review_monitor(AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        project_id,
    ))
    .await
    .expect("normal monitor update should persist");

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
        .expect("stale guard update should be rejected"));
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
        .expect("guard owner should update it"));
    assert_eq!(
        repo.get_workspace_review_monitor(&conversation_id)
            .await
            .expect("monitor should load")
            .expect("monitor should exist")
            .auto_merge_guard,
        Some(restoring_guard)
    );
}

#[tokio::test]
async fn workspace_review_auto_merge_restore_rejects_a_stale_selected_source_head() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("workspace-review-stale-source");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    repo.create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let guard = AgentWorkspaceReviewAutoMergeGuard {
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
        ProjectId::from_string("project-memory".to_string()),
    );
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.current_diff_fingerprint = Some("selected-source".to_string());
    monitor.selected_source_pull_request_number = Some(42);
    monitor.selected_source_head_sha = Some("new-head".to_string());
    monitor.auto_merge_guard = Some(guard.clone());
    repo.upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    assert!(!repo
        .complete_workspace_review_auto_merge_restore(&conversation_id, guard.clone())
        .await
        .expect("stale restore should be rejected"));
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist")
            .pr_auto_merge_current,
        Some(false)
    );
    assert_eq!(
        repo.get_workspace_review_monitor(&conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard,
        Some(guard)
    );
}

#[tokio::test]
async fn workspace_review_auto_merge_restore_rejects_a_retargeted_publication_pr() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("workspace-review-retargeted-pr");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    workspace.publication_pr_number = Some(84);
    workspace.publication_pr_status = Some("open".to_string());
    repo.create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let guard = AgentWorkspaceReviewAutoMergeGuard {
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
        ProjectId::from_string("project-memory".to_string()),
    );
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("workspace-delta".to_string());
    monitor.auto_merge_guard = Some(guard.clone());
    repo.upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    assert!(!repo
        .complete_workspace_review_auto_merge_restore(&conversation_id, guard.clone())
        .await
        .expect("retargeted restore should be rejected"));
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist")
            .pr_auto_merge_current,
        Some(false)
    );
    assert_eq!(
        repo.get_workspace_review_monitor(&conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard,
        Some(guard)
    );
}

#[tokio::test]
async fn workspace_review_auto_merge_restore_rejects_a_missing_publication_pr() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("workspace-review-missing-pr");
    let mut workspace = make_workspace(conversation_id.clone());
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    repo.create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let guard = AgentWorkspaceReviewAutoMergeGuard {
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
        ProjectId::from_string("project-memory".to_string()),
    );
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("workspace-delta".to_string());
    monitor.auto_merge_guard = Some(guard.clone());
    repo.upsert_workspace_review_monitor(monitor)
        .await
        .expect("monitor should persist");

    assert!(!repo
        .complete_workspace_review_auto_merge_restore(&conversation_id, guard.clone())
        .await
        .expect("missing PR authority should be rejected"));
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist")
            .pr_auto_merge_current,
        Some(false)
    );
    assert_eq!(
        repo.get_workspace_review_monitor(&conversation_id)
            .await
            .expect("monitor lookup should succeed")
            .expect("monitor should exist")
            .auto_merge_guard,
        Some(guard)
    );
}
#[cfg(test)]
mod tests {
    use crate::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceMode,
        AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
        AgentWorkspacePrCommentEvidenceUpsert, AgentWorkspacePrDescription,
        AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
        AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
        AgentWorkspacePrReviewMonitorStatus, ChatConversationId, IdeationAnalysisBaseRefKind,
        IdeationSessionId, PlanBranchId, ProjectId,
    };
    use crate::domain::repositories::AgentConversationWorkspaceRepository;

    use super::MemoryAgentConversationWorkspaceRepository;

    #[tokio::test]
    async fn pr_description_round_trips_and_clears() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation-1");

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
    async fn publication_events_are_listed_in_append_order() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation-1");

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
            "failed",
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
        assert_eq!(events[1].classification.as_deref(), Some("agent_fixable"));
    }

    #[tokio::test]
    async fn pr_comment_evidence_tracks_edits_inclusion_and_reads() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation-1");

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
        assert_eq!(first[0].edit_count, 0);

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
    async fn linked_ideation_session_lookup_returns_latest_workspace_and_none_for_missing() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let session_id = IdeationSessionId::from_string("ideation-session-1");
        let mut first = candidate_workspace("linked-first");
        first.linked_ideation_session_id = Some(session_id.clone());
        repo.create_or_update(first.clone()).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        let mut second = candidate_workspace("linked-second");
        second.linked_ideation_session_id = Some(session_id.clone());
        second.task_pipeline_session_id = Some(session_id.clone());
        repo.create_or_update(second.clone()).await.unwrap();

        let loaded = repo
            .get_by_linked_ideation_session_id(&session_id)
            .await
            .unwrap()
            .expect("latest linked workspace should load");
        assert_eq!(loaded.conversation_id, second.conversation_id);

        let task_pipeline = repo
            .get_by_task_pipeline_session_id(&session_id)
            .await
            .unwrap()
            .expect("durably attached Tasks workspace should load");
        assert_eq!(task_pipeline.conversation_id, second.conversation_id);

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
    async fn delete_removes_publication_events_for_conversation() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation-1");
        repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "checking",
            "started",
            "Checking workspace",
            None,
        ))
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

        let events = repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();
        assert!(events.is_empty());
        let comments = repo
            .list_pr_comment_evidence(&conversation_id, 267, 10)
            .await
            .unwrap();
        assert!(comments.is_empty());
    }

    #[tokio::test]
    async fn pr_review_monitor_and_actions_round_trip_and_clear_on_delete() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let workspace = candidate_workspace("review");
        let conversation_id = workspace.conversation_id.clone();
        repo.create_or_update(workspace).await.unwrap();

        let mut monitor = AgentWorkspacePrReviewMonitor::new(
            conversation_id.clone(),
            ProjectId::from_string("project-1".to_string()),
            411,
            Some("head-sha-1".to_string()),
        );
        monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
        monitor.monitor_enabled = true;
        monitor.first_review_completed = true;
        monitor.last_reviewed_head_sha = Some("head-sha-1".to_string());
        monitor.last_review_outcome = Some("request_changes".to_string());
        let saved_monitor = repo.upsert_pr_review_monitor(monitor).await.unwrap();
        assert_eq!(
            saved_monitor.status,
            AgentWorkspacePrReviewMonitorStatus::Watching
        );

        let loaded_monitor = repo
            .get_pr_review_monitor(&conversation_id)
            .await
            .unwrap()
            .expect("monitor should exist");
        assert!(loaded_monitor.monitor_enabled);
        assert_eq!(
            loaded_monitor.last_reviewed_head_sha.as_deref(),
            Some("head-sha-1")
        );
        assert_eq!(
            repo.list_active_pr_review_monitors().await.unwrap().len(),
            1
        );

        let action = AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            411,
            "head-sha-1".to_string(),
            AgentWorkspacePrReviewActionKind::RequestChanges,
            "Found blocking issues".to_string(),
            "Please address the blocking issues.".to_string(),
            Some(r#"[{"path":"src/lib.rs"}]"#.to_string()),
            Some("run-1".to_string()),
        );
        let saved_action = repo
            .create_or_update_pr_review_action(action)
            .await
            .unwrap();

        let replacement = AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            411,
            "head-sha-1".to_string(),
            AgentWorkspacePrReviewActionKind::Approve,
            "Looks good now".to_string(),
            "The follow-up commit fixed the review findings.".to_string(),
            None,
            Some("run-2".to_string()),
        );
        let updated_action = repo
            .create_or_update_pr_review_action(replacement)
            .await
            .unwrap();
        assert_eq!(updated_action.id, saved_action.id);
        assert_eq!(
            updated_action.proposed_action,
            AgentWorkspacePrReviewActionKind::Approve
        );
        assert_eq!(updated_action.created_by_run_id.as_deref(), Some("run-2"));

        let pending = repo
            .get_pending_pr_review_action_for_head(&conversation_id, 411, "head-sha-1")
            .await
            .unwrap()
            .expect("pending action should exist");
        assert_eq!(pending.id, saved_action.id);
        assert_eq!(
            repo.list_pr_review_actions(&conversation_id, 10)
                .await
                .unwrap()
                .len(),
            1
        );

        repo.update_pr_review_action_status(
            &saved_action.id,
            AgentWorkspacePrReviewActionStatus::Submitted,
            Some("review-1"),
        )
        .await
        .unwrap();
        let submitted = repo
            .get_pr_review_action(&saved_action.id)
            .await
            .unwrap()
            .expect("submitted action should remain queryable");
        assert_eq!(
            submitted.status,
            AgentWorkspacePrReviewActionStatus::Submitted
        );
        assert_eq!(submitted.submitted_review_id.as_deref(), Some("review-1"));
        assert!(submitted.resolved_at.is_some());
        assert!(repo
            .get_pending_pr_review_action_for_head(&conversation_id, 411, "head-sha-1")
            .await
            .unwrap()
            .is_none());

        let mut terminal_monitor = loaded_monitor;
        terminal_monitor.status = AgentWorkspacePrReviewMonitorStatus::Terminal;
        repo.upsert_pr_review_monitor(terminal_monitor)
            .await
            .unwrap();
        assert!(repo
            .list_active_pr_review_monitors()
            .await
            .unwrap()
            .is_empty());

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

    fn candidate_workspace(id: &str) -> AgentConversationWorkspace {
        AgentConversationWorkspace::new(
            ChatConversationId::new(),
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-sha".to_string()),
            format!("ralphx/demo/agent-{id}"),
            format!("/tmp/ralphx-demo-{id}"),
        )
    }

    #[tokio::test]
    async fn active_direct_published_workspaces_include_refreshed_prs_for_status_polling() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let mut pushed = candidate_workspace("pushed");
        pushed.publication_pr_number = Some(12);
        pushed.publication_pr_status = Some("open".to_string());
        pushed.publication_push_status = Some("pushed".to_string());
        let mut refreshed = candidate_workspace("refreshed");
        refreshed.publication_pr_number = Some(13);
        refreshed.publication_pr_status = Some("open".to_string());
        refreshed.publication_push_status = Some("refreshed".to_string());

        repo.create_or_update(pushed.clone()).await.unwrap();
        repo.create_or_update(refreshed.clone()).await.unwrap();

        let workspaces = repo
            .list_active_direct_published_workspaces()
            .await
            .unwrap();

        assert_eq!(workspaces.len(), 2);
        assert!(workspaces
            .iter()
            .any(|workspace| workspace.conversation_id == pushed.conversation_id));
        assert!(workspaces
            .iter()
            .any(|workspace| workspace.conversation_id == refreshed.conversation_id));
    }

    #[tokio::test]
    async fn active_unpublished_edit_workspaces_filters_to_unpublished_open_edit_workspaces() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let unpublished = candidate_workspace("unpublished");
        repo.create_or_update(unpublished.clone()).await.unwrap();

        let mut published = candidate_workspace("published");
        published.publication_pr_number = Some(72);
        published.publication_pr_status = Some("open".to_string());
        repo.create_or_update(published).await.unwrap();

        let mut ideation = candidate_workspace("ideation");
        ideation.mode = AgentConversationWorkspaceMode::Ideation;
        repo.create_or_update(ideation).await.unwrap();

        let mut execution_owned = candidate_workspace("execution-owned");
        execution_owned.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-branch-1"));
        repo.create_or_update(execution_owned).await.unwrap();

        let mut archived = candidate_workspace("archived");
        archived.status = AgentConversationWorkspaceStatus::Archived;
        repo.create_or_update(archived).await.unwrap();

        let workspaces = repo
            .list_active_unpublished_edit_workspaces()
            .await
            .unwrap();

        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].conversation_id, unpublished.conversation_id);
    }

    #[tokio::test]
    async fn stale_base_detected_at_round_trips_through_create_or_update() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let mut workspace = candidate_workspace("stale-base");
        let conversation_id = workspace.conversation_id;
        let detected_at = "2026-08-06T15:00:00+00:00"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap();
        workspace.stale_base_detected_at = Some(detected_at);
        repo.create_or_update(workspace).await.unwrap();

        let loaded = repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should exist");
        assert_eq!(loaded.stale_base_detected_at, Some(detected_at));

        let mut cleared = loaded;
        cleared.stale_base_detected_at = None;
        repo.create_or_update(cleared).await.unwrap();

        let reloaded = repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should exist");
        assert_eq!(reloaded.stale_base_detected_at, None);
    }

    #[tokio::test]
    async fn set_stale_base_detected_at_round_trips_via_targeted_setter() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let workspace = candidate_workspace("stale-base-setter");
        let conversation_id = workspace.conversation_id;
        repo.create_or_update(workspace).await.unwrap();

        let detected_at = "2026-08-06T15:00:00+00:00"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap();
        repo.set_stale_base_detected_at(&conversation_id, Some(detected_at))
            .await
            .unwrap();
        let loaded = repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should exist");
        assert_eq!(loaded.stale_base_detected_at, Some(detected_at));

        repo.set_stale_base_detected_at(&conversation_id, None)
            .await
            .unwrap();
        let cleared = repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should exist");
        assert_eq!(cleared.stale_base_detected_at, None);
    }

    #[tokio::test]
    async fn set_stale_base_detected_at_is_a_no_op_for_a_missing_conversation() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let missing_id = ChatConversationId::new();

        repo.set_stale_base_detected_at(&missing_id, Some(chrono::Utc::now()))
            .await
            .unwrap();

        assert!(repo
            .get_by_conversation_id(&missing_id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn update_publication_clears_stale_base_detected_at_when_pr_number_is_set() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let mut workspace = candidate_workspace("stale-base-publish");
        let conversation_id = workspace.conversation_id;
        let detected_at = "2026-08-06T15:00:00+00:00"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap();
        workspace.stale_base_detected_at = Some(detected_at);
        repo.create_or_update(workspace).await.unwrap();

        repo.update_publication(
            &conversation_id,
            Some(91),
            Some("https://github.com/owner/repo/pull/91"),
            Some("open"),
            Some("pushed"),
        )
        .await
        .unwrap();

        let updated = repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should exist");
        assert_eq!(updated.publication_pr_number, Some(91));
        assert_eq!(updated.stale_base_detected_at, None);
    }

    #[tokio::test]
    async fn update_publication_leaves_stale_base_detected_at_untouched_when_pr_number_is_none() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let mut workspace = candidate_workspace("stale-base-publish-none");
        let conversation_id = workspace.conversation_id;
        let detected_at = "2026-08-06T15:00:00+00:00"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap();
        workspace.stale_base_detected_at = Some(detected_at);
        repo.create_or_update(workspace).await.unwrap();

        repo.update_publication(&conversation_id, None, None, None, None)
            .await
            .unwrap();

        let updated = repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should exist");
        assert_eq!(updated.publication_pr_number, None);
        assert_eq!(updated.stale_base_detected_at, Some(detected_at));
    }

    #[tokio::test]
    async fn transient_publish_status_workspaces_filter_stale_active_open_rows() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let stale = chrono::Utc::now() - chrono::Duration::minutes(10);

        let mut refreshing = candidate_workspace("refreshing");
        refreshing.publication_pr_number = Some(21);
        refreshing.publication_pr_status = Some("open".to_string());
        refreshing.publication_push_status = Some("refreshing".to_string());
        refreshing.updated_at = stale;

        let mut pending = candidate_workspace("redrive-pending");
        pending.publication_pr_number = Some(22);
        pending.publication_pr_status = Some("open".to_string());
        pending.publication_push_status = Some("redrive_pending".to_string());
        pending.updated_at = stale;

        let mut delivering = candidate_workspace("redrive-delivering");
        delivering.publication_pr_number = Some(25);
        delivering.publication_pr_status = Some("open".to_string());
        delivering.publication_push_status = Some("redrive_delivering".to_string());
        delivering.updated_at = stale;

        let mut closed = candidate_workspace("closed");
        closed.publication_pr_number = Some(23);
        closed.publication_pr_status = Some("closed".to_string());
        closed.publication_push_status = Some("committing".to_string());
        closed.updated_at = stale;

        let mut archived = candidate_workspace("archived-transient");
        archived.status = AgentConversationWorkspaceStatus::Archived;
        archived.publication_pr_number = Some(24);
        archived.publication_pr_status = Some("open".to_string());
        archived.publication_push_status = Some("describing".to_string());
        archived.updated_at = stale;

        for workspace in [
            refreshing.clone(),
            pending.clone(),
            delivering.clone(),
            closed,
            archived,
        ] {
            repo.create_or_update(workspace).await.unwrap();
        }

        let workspaces = repo
            .list_active_transient_publish_status_workspaces(0)
            .await
            .unwrap();

        assert_eq!(workspaces.len(), 3);
        assert!(workspaces
            .iter()
            .any(|workspace| workspace.conversation_id == refreshing.conversation_id));
        assert!(workspaces
            .iter()
            .any(|workspace| workspace.conversation_id == pending.conversation_id));
        assert!(workspaces
            .iter()
            .any(|workspace| workspace.conversation_id == delivering.conversation_id));
    }

    #[tokio::test]
    async fn pr_poller_recovery_workspaces_include_supervised_ideation_prs() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let mut direct = candidate_workspace("direct");
        direct.publication_pr_number = Some(12);
        direct.publication_pr_status = Some("open".to_string());
        direct.publication_push_status = Some("pushed".to_string());

        let mut ideation = candidate_workspace("ideation");
        ideation.mode = AgentConversationWorkspaceMode::Ideation;
        ideation.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-1"));
        ideation.publication_pr_number = Some(13);
        ideation.publication_pr_status = Some("open".to_string());
        ideation.publication_push_status = Some("pushed".to_string());
        ideation.pr_autofix_enabled = true;

        let mut unsupervised_ideation = candidate_workspace("unsupervised-ideation");
        unsupervised_ideation.mode = AgentConversationWorkspaceMode::Ideation;
        unsupervised_ideation.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-2"));
        unsupervised_ideation.publication_pr_number = Some(14);
        unsupervised_ideation.publication_pr_status = Some("open".to_string());
        unsupervised_ideation.publication_push_status = Some("pushed".to_string());

        for workspace in [
            direct.clone(),
            ideation.clone(),
            unsupervised_ideation.clone(),
        ] {
            repo.create_or_update(workspace).await.unwrap();
        }

        let workspaces = repo
            .list_active_pr_poller_recovery_workspaces()
            .await
            .unwrap();

        assert_eq!(workspaces.len(), 2);
        assert!(workspaces
            .iter()
            .any(|workspace| workspace.conversation_id == direct.conversation_id));
        assert!(workspaces
            .iter()
            .any(|workspace| workspace.conversation_id == ideation.conversation_id));
    }

    #[tokio::test]
    async fn external_pr_reconciliation_candidates_filter_and_limit_recent_direct_workspaces() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();

        let first = candidate_workspace("candidate-1");
        let second = candidate_workspace("candidate-2");
        let mut linked_failed = candidate_workspace("linked-failed");
        linked_failed.publication_pr_number = Some(12);
        linked_failed.publication_pr_status = Some("open".to_string());
        linked_failed.publication_push_status = Some("failed".to_string());
        let mut linked_missing = candidate_workspace("linked-missing");
        linked_missing.status = AgentConversationWorkspaceStatus::Missing;
        linked_missing.publication_pr_number = Some(13);
        linked_missing.publication_pr_status = Some("open".to_string());
        linked_missing.publication_push_status = Some("needs_agent".to_string());
        let mut terminal_linked = candidate_workspace("terminal-linked");
        terminal_linked.publication_pr_number = Some(14);
        terminal_linked.publication_pr_status = Some("merged".to_string());
        terminal_linked.publication_push_status = Some("pushed".to_string());
        let mut linked_plan = candidate_workspace("linked-plan");
        linked_plan.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-1"));
        let mut blocked_push = candidate_workspace("blocked-push");
        blocked_push.publication_push_status = Some("needs_agent".to_string());
        let mut terminal = candidate_workspace("terminal");
        terminal.publication_pr_status = Some("merged".to_string());
        let mut chat = candidate_workspace("chat");
        chat.mode = AgentConversationWorkspaceMode::Chat;
        let mut archived = candidate_workspace("archived");
        archived.status = AgentConversationWorkspaceStatus::Archived;

        for workspace in [
            first.clone(),
            second.clone(),
            linked_failed.clone(),
            linked_missing.clone(),
            terminal_linked.clone(),
            linked_plan,
            blocked_push,
            terminal,
            chat,
            archived,
        ] {
            repo.create_or_update(workspace).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }

        let limited = repo
            .list_active_direct_external_pr_reconciliation_candidates(1)
            .await
            .unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].conversation_id, terminal_linked.conversation_id);

        let all = repo
            .list_active_direct_external_pr_reconciliation_candidates(10)
            .await
            .unwrap();
        assert_eq!(
            all.into_iter()
                .map(|workspace| workspace.conversation_id)
                .collect::<Vec<_>>(),
            vec![
                terminal_linked.conversation_id,
                linked_missing.conversation_id,
                linked_failed.conversation_id,
                second.conversation_id,
                first.conversation_id
            ]
        );
    }

    #[tokio::test]
    async fn pr_supervision_recovery_candidates_filter_blocked_failed_supervised_prs() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();

        let mut first = candidate_workspace("candidate-1");
        first.publication_pr_number = Some(41);
        first.publication_pr_status = Some("open".to_string());
        first.publication_push_status = Some("failed".to_string());
        first.pr_supervision_status = Some("blocked".to_string());
        first.pr_autofix_enabled = true;
        let mut second = candidate_workspace("candidate-2");
        second.publication_pr_number = Some(42);
        second.publication_pr_status = Some("open".to_string());
        second.publication_push_status = Some("failed".to_string());
        second.pr_supervision_status = Some("blocked".to_string());
        second.pr_auto_merge_desired = true;
        let mut disabled = candidate_workspace("disabled");
        disabled.publication_pr_number = Some(43);
        disabled.publication_push_status = Some("failed".to_string());
        disabled.pr_supervision_status = Some("blocked".to_string());
        let mut needs_agent = candidate_workspace("needs-agent");
        needs_agent.publication_pr_number = Some(44);
        needs_agent.publication_push_status = Some("needs_agent".to_string());
        needs_agent.pr_supervision_status = Some("blocked".to_string());
        needs_agent.pr_autofix_enabled = true;
        let mut terminal = candidate_workspace("terminal");
        terminal.publication_pr_number = Some(45);
        terminal.publication_pr_status = Some("merged".to_string());
        terminal.publication_push_status = Some("failed".to_string());
        terminal.pr_supervision_status = Some("blocked".to_string());
        terminal.pr_autofix_enabled = true;
        let mut review_handoff = candidate_workspace("review-handoff");
        review_handoff.publication_pr_number = Some(46);
        review_handoff.publication_pr_status = Some("open".to_string());
        review_handoff.publication_push_status = Some("refreshed".to_string());
        review_handoff.pr_supervision_status = Some("reviewing".to_string());
        review_handoff.pr_autofix_enabled = true;
        let mut stranded_fix = candidate_workspace("stranded-fix");
        stranded_fix.publication_pr_number = Some(47);
        stranded_fix.publication_pr_status = Some("open".to_string());
        stranded_fix.publication_push_status = Some("refreshed".to_string());
        stranded_fix.pr_supervision_status = Some("fixing".to_string());
        stranded_fix.pr_autofix_enabled = true;

        for workspace in [
            first.clone(),
            second.clone(),
            disabled,
            needs_agent,
            terminal,
            review_handoff.clone(),
            stranded_fix.clone(),
        ] {
            repo.create_or_update(workspace).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }

        let limited = repo
            .list_active_direct_pr_supervision_recovery_candidates(1)
            .await
            .unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].conversation_id, stranded_fix.conversation_id);

        let all = repo
            .list_active_direct_pr_supervision_recovery_candidates(10)
            .await
            .unwrap();
        assert_eq!(
            all.into_iter()
                .map(|workspace| workspace.conversation_id)
                .collect::<Vec<_>>(),
            vec![
                stranded_fix.conversation_id,
                review_handoff.conversation_id,
                second.conversation_id,
                first.conversation_id
            ]
        );
    }

    #[tokio::test]
    async fn linked_plan_pr_supervision_recovery_candidates_filter_ideation_rows() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();

        let mut blocked = candidate_workspace("linked-blocked");
        blocked.mode = AgentConversationWorkspaceMode::Ideation;
        blocked.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-linked-1"));
        blocked.pr_supervision_status = Some("blocked".to_string());
        blocked.pr_autofix_enabled = true;

        let mut fixing = candidate_workspace("linked-fixing");
        fixing.mode = AgentConversationWorkspaceMode::Ideation;
        fixing.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-linked-2"));
        fixing.pr_supervision_status = Some("fixing".to_string());
        fixing.pr_auto_merge_desired = true;

        let mut direct = candidate_workspace("direct");
        direct.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-direct"));
        direct.pr_supervision_status = Some("blocked".to_string());
        direct.pr_autofix_enabled = true;

        let mut unlinked = candidate_workspace("unlinked");
        unlinked.mode = AgentConversationWorkspaceMode::Ideation;
        unlinked.pr_supervision_status = Some("blocked".to_string());
        unlinked.pr_autofix_enabled = true;

        let mut disabled = candidate_workspace("disabled");
        disabled.mode = AgentConversationWorkspaceMode::Ideation;
        disabled.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-disabled"));
        disabled.pr_supervision_status = Some("blocked".to_string());

        let mut monitoring = candidate_workspace("monitoring");
        monitoring.mode = AgentConversationWorkspaceMode::Ideation;
        monitoring.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-monitoring"));
        monitoring.pr_supervision_status = Some("monitoring".to_string());
        monitoring.pr_autofix_enabled = true;

        let mut paused = candidate_workspace("paused");
        paused.mode = AgentConversationWorkspaceMode::Ideation;
        paused.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-paused"));
        paused.pr_supervision_status = Some("blocked".to_string());
        paused.pr_autofix_enabled = true;
        paused.auto_publish_enabled = false;

        for workspace in [
            blocked.clone(),
            fixing.clone(),
            direct,
            unlinked,
            disabled,
            monitoring,
            paused,
        ] {
            repo.create_or_update(workspace).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }

        let limited = repo
            .list_active_linked_plan_pr_supervision_recovery_candidates(1)
            .await
            .unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].conversation_id, fixing.conversation_id);

        let all = repo
            .list_active_linked_plan_pr_supervision_recovery_candidates(10)
            .await
            .unwrap();
        assert_eq!(
            all.into_iter()
                .map(|workspace| workspace.conversation_id)
                .collect::<Vec<_>>(),
            vec![fixing.conversation_id, blocked.conversation_id]
        );
    }
}
