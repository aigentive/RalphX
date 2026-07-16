use crate::application::agent_workspace_review::ensure_workspace_review_run_is_active;
use crate::domain::entities::{
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewTargetScope, ChatConversationId, ProjectId,
};

fn active_monitor() -> AgentWorkspaceReviewMonitor {
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        ChatConversationId::from_string("review-conversation".to_string()),
        ProjectId::from_string("review-project".to_string()),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("diff-current".to_string());
    monitor.last_run_id = Some("run-current".to_string());
    monitor
}

#[test]
fn current_review_run_can_mutate_review_state() {
    ensure_workspace_review_run_is_active(
        &active_monitor(),
        Some("run-current"),
        "write Review artifact",
    )
    .expect("the current active reviewer owns review mutations");
}

#[test]
fn terminal_and_stale_review_runs_cannot_mutate_review_state() {
    let mut terminal = active_monitor();
    terminal.status = AgentWorkspaceReviewMonitorStatus::Ready;
    let terminal_error = ensure_workspace_review_run_is_active(
        &terminal,
        Some("run-current"),
        "write Review artifact",
    )
    .expect_err("terminal reviewer follow-ups must be read-only");
    assert!(terminal_error.to_string().contains("current active"));

    let stale_error = ensure_workspace_review_run_is_active(
        &active_monitor(),
        Some("run-stale"),
        "complete Review",
    )
    .expect_err("stale reviewer runs must not mutate current Review state");
    assert!(stale_error.to_string().contains("does not match"));
}
