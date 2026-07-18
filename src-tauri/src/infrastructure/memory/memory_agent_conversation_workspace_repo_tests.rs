use super::MemoryAgentConversationWorkspaceRepository;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, AgentWorkspaceReviewApprovalSnapshot,
    AgentWorkspaceReviewAutoMergeGuard, AgentWorkspaceReviewAutoMergeGuardStatus,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewOutcome, AgentWorkspaceReviewTargetScope, ArtifactId, ChatConversationId,
    IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranchId, ProjectId,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::domain::repositories::AgentWorkspaceLocalCleanupClaim;

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
