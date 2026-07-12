use super::{
    AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor, ArtifactId,
    ChatConversationId, ProjectId,
};

fn review_pair() -> (AgentWorkspacePrReviewMonitor, AgentWorkspacePrReviewAction) {
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        702,
        Some("head-a".to_string()),
    );
    monitor.first_action_resolved = true;
    monitor.last_review_run_id = Some("review-run-1".to_string());
    monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-1"));
    monitor.review_artifact_head_sha = Some("head-a".to_string());

    let action = AgentWorkspacePrReviewAction::new(
        conversation_id,
        702,
        "head-a".to_string(),
        AgentWorkspacePrReviewActionKind::Approve,
        "passes".to_string(),
        "approved".to_string(),
        None,
        Some("review-run-1".to_string()),
    );

    (monitor, action)
}

#[test]
fn pr_review_auto_approve_requires_every_safety_gate() {
    let (monitor, action) = review_pair();
    assert!(monitor.can_auto_approve(&action));

    let mut disabled = monitor.clone();
    disabled.auto_approve_enabled = false;
    assert!(!disabled.can_auto_approve(&action));

    let mut unresolved = monitor.clone();
    unresolved.first_action_resolved = false;
    assert!(!unresolved.can_auto_approve(&action));

    let mut request_changes = action.clone();
    request_changes.proposed_action = AgentWorkspacePrReviewActionKind::RequestChanges;
    assert!(!monitor.can_auto_approve(&request_changes));

    let mut already_submitting = action.clone();
    already_submitting.status = AgentWorkspacePrReviewActionStatus::Submitting;
    assert!(!monitor.can_auto_approve(&already_submitting));

    let mut manual_action = action.clone();
    manual_action.created_by_run_id = None;
    assert!(!monitor.can_auto_approve(&manual_action));

    let mut stale_run = monitor.clone();
    stale_run.last_review_run_id = Some("older-run".to_string());
    assert!(!stale_run.can_auto_approve(&action));

    let mut missing_artifact = monitor.clone();
    missing_artifact.review_artifact_id = None;
    assert!(!missing_artifact.can_auto_approve(&action));

    let mut stale_artifact = monitor;
    stale_artifact.review_artifact_head_sha = Some("head-b".to_string());
    assert!(!stale_artifact.can_auto_approve(&action));
}
