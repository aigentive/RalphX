use super::MemoryAgentConversationWorkspaceRepository;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, ChatConversationId, IdeationAnalysisBaseRefKind,
    IdeationSessionId, PlanBranchId, ProjectId,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;

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

    repo.supersede_pending_pr_review_actions_except_head(&conversation_id, 703, "current-head")
        .await
        .expect("supersede old pending actions");

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
