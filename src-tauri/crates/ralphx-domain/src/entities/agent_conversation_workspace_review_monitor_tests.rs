use super::{
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewOutcome, AgentWorkspaceReviewTargetScope, ArtifactId, ChatConversationId,
    ProjectId,
};

#[test]
fn workspace_review_monitor_defaults_and_currentness_are_explicit() {
    let conversation_id = ChatConversationId::from_string("review-monitor-conversation");
    let project_id = ProjectId::from_string("project-1".to_string());
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id);

    assert_eq!(monitor.conversation_id, conversation_id);
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Idle);
    assert_eq!(monitor.review_outcome, AgentWorkspaceReviewOutcome::None);
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::NotRequired
    );
    assert!(monitor.current_target_scope.is_none());
    assert!(monitor.review_conversation_id.is_none());
    assert!(monitor.review_artifact_id.is_none());
    assert!(!monitor.is_current_for_target(
        AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        Some("head"),
        "fingerprint"
    ));

    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.reviewed_head_sha = Some("head".to_string());
    monitor.reviewed_diff_fingerprint = Some("fingerprint".to_string());
    monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-1"));
    monitor.review_artifact_version = Some(1);
    monitor.review_requested_changes_artifact_id = Some(ArtifactId::from_string("artifact-2"));
    monitor.review_requested_changes_artifact_version = Some(1);

    assert!(monitor.is_current_for_target(
        AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        Some("head"),
        "fingerprint"
    ));
    assert!(!monitor.has_current_passing_review_for_target(
        AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        Some("head"),
        "fingerprint"
    ));
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    assert!(monitor.has_current_passing_review_for_target(
        AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        Some("head"),
        "fingerprint"
    ));
    assert!(monitor.is_current_for_target(
        AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        Some("different-head"),
        "fingerprint"
    ));
    assert!(monitor.has_current_passing_review_for_target(
        AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        Some("different-head"),
        "fingerprint"
    ));
    assert!(!monitor.is_current_for_target(
        AgentWorkspaceReviewTargetScope::SelectedSource,
        Some("head"),
        "fingerprint"
    ));
    assert!(!monitor.is_current_for_target(
        AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        Some("head"),
        "different-fingerprint"
    ));

    monitor.reviewed_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    assert!(monitor.is_current_for_target(
        AgentWorkspaceReviewTargetScope::SelectedSource,
        Some("head"),
        "fingerprint"
    ));
    assert!(!monitor.is_current_for_target(
        AgentWorkspaceReviewTargetScope::SelectedSource,
        Some("different-head"),
        "fingerprint"
    ));
}
