use crate::application::agent_workspace_review::{
    classify_workspace_review_runtime_authority, ensure_workspace_review_run_is_active,
};
use crate::domain::entities::{
    AgentRun, AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewRuntimeState, AgentWorkspaceReviewTargetScope, ChatConversationId,
    ProjectId,
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

#[test]
fn runtime_authority_is_independent_from_artifact_freshness() {
    let review_conversation_id = ChatConversationId::new();
    let run = AgentRun::new(review_conversation_id);
    let run_id = run.id.to_string();
    let conversation_id = review_conversation_id.as_str();
    let mut monitor = active_monitor();
    monitor.review_conversation_id = Some(review_conversation_id);
    monitor.last_run_id = Some(run_id.clone());

    let authority = classify_workspace_review_runtime_authority(
        &monitor,
        Some(&run_id),
        Some(&conversation_id),
        Some(&run),
    );

    assert!(authority.can_mutate_review_state);
    assert_eq!(
        authority.review_runtime_state,
        AgentWorkspaceReviewRuntimeState::ActiveOwned
    );
}

#[test]
fn runtime_authority_fails_closed_for_terminal_missing_malformed_and_stale_callers() {
    let review_conversation_id = ChatConversationId::new();
    let run = AgentRun::new(review_conversation_id);
    let run_id = run.id.to_string();
    let conversation_id = review_conversation_id.as_str();
    let mut monitor = active_monitor();
    monitor.review_conversation_id = Some(review_conversation_id);
    monitor.last_run_id = Some(run_id.clone());

    let cases = [
        (
            classify_workspace_review_runtime_authority(&monitor, None, None, None),
            AgentWorkspaceReviewRuntimeState::MissingRuntimeIdentity,
        ),
        (
            classify_workspace_review_runtime_authority(
                &monitor,
                Some("not-a-run-id"),
                Some(&conversation_id),
                None,
            ),
            AgentWorkspaceReviewRuntimeState::MalformedRuntimeIdentity,
        ),
        (
            classify_workspace_review_runtime_authority(
                &monitor,
                Some(&crate::domain::entities::AgentRunId::new().to_string()),
                Some(&conversation_id),
                None,
            ),
            AgentWorkspaceReviewRuntimeState::StaleRuntime,
        ),
        (
            classify_workspace_review_runtime_authority(
                &monitor,
                Some(&run_id),
                Some(&ChatConversationId::new().as_str()),
                Some(&run),
            ),
            AgentWorkspaceReviewRuntimeState::StaleRuntime,
        ),
    ];
    for (authority, expected_state) in cases {
        assert!(!authority.can_mutate_review_state);
        assert_eq!(authority.review_runtime_state, expected_state);
    }

    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    let terminal = classify_workspace_review_runtime_authority(
        &monitor,
        Some(&run_id),
        Some(&conversation_id),
        Some(&run),
    );
    assert!(!terminal.can_mutate_review_state);
    assert_eq!(
        terminal.review_runtime_state,
        AgentWorkspaceReviewRuntimeState::Terminal
    );
}
