use super::{
    AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind, AgentWorkspacePrReviewMonitor,
    AgentWorkspaceReviewAutoMergeGuardStatus, ArtifactId, ChatConversationId, ProjectId,
};

fn monitor_and_action() -> (AgentWorkspacePrReviewMonitor, AgentWorkspacePrReviewAction) {
    let conversation_id = ChatConversationId::new();
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        ProjectId("project-1".to_string()),
        42,
        Some("head-1".to_string()),
    );
    let action = AgentWorkspacePrReviewAction::new(
        conversation_id,
        42,
        "head-1".to_string(),
        AgentWorkspacePrReviewActionKind::Approve,
        "review passed".to_string(),
        "Looks good.".to_string(),
        None,
        Some("run-1".to_string()),
    );
    monitor.last_review_run_id = Some("run-1".to_string());
    monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-1".to_string()));
    monitor.review_artifact_head_sha = Some("head-1".to_string());
    (monitor, action)
}

#[test]
fn auto_approval_defaults_on_but_requires_a_resolved_first_action() {
    let (monitor, action) = monitor_and_action();

    assert!(monitor.auto_approve_enabled);
    assert!(!monitor.first_action_resolved);
    assert!(!monitor.can_auto_approve(&action));
}

#[test]
fn auto_approval_requires_current_approve_artifact_and_run() {
    let (mut monitor, action) = monitor_and_action();
    monitor.first_action_resolved = true;

    assert!(monitor.can_auto_approve(&action));

    monitor.review_artifact_head_sha = Some("other-head".to_string());
    assert!(!monitor.can_auto_approve(&action));

    monitor.review_artifact_head_sha = Some("head-1".to_string());
    monitor.last_review_run_id = Some("other-run".to_string());
    assert!(!monitor.can_auto_approve(&action));

    monitor.last_review_run_id = Some("run-1".to_string());
    monitor.auto_approve_enabled = false;
    assert!(!monitor.can_auto_approve(&action));
}

#[test]
fn workspace_review_auto_merge_guard_status_round_trips_persisted_values() {
    for (status, persisted) in [
        (AgentWorkspaceReviewAutoMergeGuardStatus::Pausing, "pausing"),
        (
            AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
            "paused_for_review",
        ),
        (
            AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish,
            "awaiting_publish",
        ),
        (
            AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
            "restoring",
        ),
        (
            AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed,
            "restore_failed",
        ),
    ] {
        assert_eq!(status.to_string(), persisted);
        assert_eq!(persisted.parse(), Ok(status));
    }

    assert_eq!(
        "unknown".parse::<AgentWorkspaceReviewAutoMergeGuardStatus>(),
        Err("unknown workspace review auto-merge guard status: 'unknown'".to_string())
    );
}
