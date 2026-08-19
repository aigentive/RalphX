use chrono::Utc;

use crate::commands::agent_sidebar_review_state::{
    lifecycle_monitor_for_sidebar, pr_review_state_for_row, SidebarPrReviewLaneBucket,
    SidebarPrReviewState, PR_REVIEW_OUTCOME_NO_ACTION, PR_REVIEW_OUTCOME_SKIPPED,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentRunStatus, AgentWorkspacePrReviewMonitor, AgentWorkspacePrReviewMonitorStatus,
    ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
};

fn monitor(
    status: AgentWorkspacePrReviewMonitorStatus,
    last_review_outcome: Option<&str>,
) -> AgentWorkspacePrReviewMonitor {
    let now = Utc::now();
    AgentWorkspacePrReviewMonitor {
        conversation_id: ChatConversationId::from_string("conversation-1"),
        project_id: ProjectId::from_string("project-1".to_string()),
        pr_number: 42,
        status,
        monitor_enabled: true,
        auto_approve_enabled: false,
        first_review_completed: true,
        first_action_resolved: true,
        last_seen_head_sha: None,
        last_reviewed_head_sha: None,
        last_review_run_id: None,
        last_review_outcome: last_review_outcome.map(str::to_string),
        last_submitted_review_id: None,
        review_artifact_id: None,
        review_artifact_head_sha: None,
        review_artifact_version: None,
        review_artifact_updated_at: None,
        last_error: None,
        created_at: now,
        updated_at: now,
    }
}

fn review_pr_workspace() -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-1"),
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::ReviewPr,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "ralphx/project/agent-conversation-1".to_string(),
        "/tmp/worktrees/agent-conversation-1".to_string(),
    )
}

// ---------------------------------------------------------------------------
// Derivation table (Step 1) — one case per row, first match wins.
// ---------------------------------------------------------------------------

#[test]
fn missing_monitor_yields_none_so_the_caller_falls_back_to_legacy_lanes() {
    assert_eq!(pr_review_state_for_row(None, None), None);
    assert_eq!(
        pr_review_state_for_row(None, Some(AgentRunStatus::Running)),
        None
    );
}

#[test]
fn terminal_monitor_yields_none_so_terminal_settlement_keeps_ownership() {
    let monitor = monitor(
        AgentWorkspacePrReviewMonitorStatus::Terminal,
        Some("approve"),
    );
    assert_eq!(pr_review_state_for_row(Some(&monitor), None), None);
}

#[test]
fn running_agent_outranks_a_resting_monitor_status() {
    let monitor = monitor(
        AgentWorkspacePrReviewMonitorStatus::Watching,
        Some("approve"),
    );
    assert_eq!(
        pr_review_state_for_row(Some(&monitor), Some(AgentRunStatus::Running)),
        Some(SidebarPrReviewState::Reviewing)
    );
}

#[test]
fn reviewing_status_yields_reviewing() {
    let monitor = monitor(AgentWorkspacePrReviewMonitorStatus::Reviewing, None);
    assert_eq!(
        pr_review_state_for_row(Some(&monitor), None),
        Some(SidebarPrReviewState::Reviewing)
    );
}

#[test]
fn submitting_status_yields_submitting() {
    let monitor = monitor(
        AgentWorkspacePrReviewMonitorStatus::Submitting,
        Some("approve"),
    );
    assert_eq!(
        pr_review_state_for_row(Some(&monitor), None),
        Some(SidebarPrReviewState::Submitting)
    );
}

#[test]
fn awaiting_user_with_diverged_head_shas_yields_head_moved() {
    let mut monitor = monitor(
        AgentWorkspacePrReviewMonitorStatus::AwaitingUser,
        Some("approve"),
    );
    monitor.last_reviewed_head_sha = Some("aaa111".to_string());
    monitor.last_seen_head_sha = Some("bbb222".to_string());
    assert_eq!(
        pr_review_state_for_row(Some(&monitor), None),
        Some(SidebarPrReviewState::HeadMoved)
    );
}

#[test]
fn awaiting_user_with_matching_head_shas_does_not_yield_head_moved() {
    let mut monitor = monitor(
        AgentWorkspacePrReviewMonitorStatus::AwaitingUser,
        Some("approve"),
    );
    monitor.last_reviewed_head_sha = Some("aaa111".to_string());
    monitor.last_seen_head_sha = Some("aaa111".to_string());
    assert_eq!(
        pr_review_state_for_row(Some(&monitor), None),
        Some(SidebarPrReviewState::NeedsApproval)
    );
}

#[test]
fn awaiting_user_with_one_missing_head_sha_does_not_yield_head_moved() {
    let mut monitor = monitor(
        AgentWorkspacePrReviewMonitorStatus::AwaitingUser,
        Some("comment"),
    );
    monitor.last_reviewed_head_sha = None;
    monitor.last_seen_head_sha = Some("bbb222".to_string());
    assert_eq!(
        pr_review_state_for_row(Some(&monitor), None),
        Some(SidebarPrReviewState::NeedsDecisionComment)
    );
}

#[test]
fn awaiting_user_maps_each_proposed_action_kind_to_its_needs_variant() {
    for (outcome, expected) in [
        ("approve", SidebarPrReviewState::NeedsApproval),
        (
            "request_changes",
            SidebarPrReviewState::NeedsDecisionChanges,
        ),
        ("comment", SidebarPrReviewState::NeedsDecisionComment),
    ] {
        let monitor = monitor(
            AgentWorkspacePrReviewMonitorStatus::AwaitingUser,
            Some(outcome),
        );
        assert_eq!(
            pr_review_state_for_row(Some(&monitor), None),
            Some(expected),
            "outcome {outcome}"
        );
    }
}

#[test]
fn awaiting_user_with_unknown_or_missing_outcome_yields_generic_needs_decision() {
    for outcome in [None, Some("something_new"), Some(PR_REVIEW_OUTCOME_SKIPPED)] {
        let monitor = monitor(AgentWorkspacePrReviewMonitorStatus::AwaitingUser, outcome);
        assert_eq!(
            pr_review_state_for_row(Some(&monitor), None),
            Some(SidebarPrReviewState::NeedsDecision),
            "outcome {outcome:?}"
        );
    }
}

#[test]
fn blocked_status_yields_blocked() {
    let monitor = monitor(
        AgentWorkspacePrReviewMonitorStatus::Blocked,
        Some("approve"),
    );
    assert_eq!(
        pr_review_state_for_row(Some(&monitor), None),
        Some(SidebarPrReviewState::Blocked)
    );
}

#[test]
fn watching_maps_each_submitted_action_kind_to_its_resting_variant() {
    for (outcome, expected) in [
        ("approve", SidebarPrReviewState::Approved),
        ("request_changes", SidebarPrReviewState::ChangesRequested),
        ("comment", SidebarPrReviewState::Commented),
    ] {
        let monitor = monitor(AgentWorkspacePrReviewMonitorStatus::Watching, Some(outcome));
        assert_eq!(
            pr_review_state_for_row(Some(&monitor), None),
            Some(expected),
            "outcome {outcome}"
        );
    }
}

#[test]
fn watching_with_skipped_no_action_unknown_or_missing_outcome_yields_generic_watching() {
    for outcome in [
        None,
        Some(PR_REVIEW_OUTCOME_SKIPPED),
        Some(PR_REVIEW_OUTCOME_NO_ACTION),
        Some("something_new"),
    ] {
        let monitor = monitor(AgentWorkspacePrReviewMonitorStatus::Watching, outcome);
        assert_eq!(
            pr_review_state_for_row(Some(&monitor), None),
            Some(SidebarPrReviewState::Watching),
            "outcome {outcome:?}"
        );
    }
}

#[test]
fn paused_status_yields_paused() {
    let monitor = monitor(AgentWorkspacePrReviewMonitorStatus::Paused, Some("approve"));
    assert_eq!(
        pr_review_state_for_row(Some(&monitor), None),
        Some(SidebarPrReviewState::Paused)
    );
}

#[test]
fn idle_status_rests_rather_than_asking_for_attention() {
    let monitor = monitor(AgentWorkspacePrReviewMonitorStatus::Idle, None);
    assert_eq!(
        pr_review_state_for_row(Some(&monitor), None),
        Some(SidebarPrReviewState::Watching)
    );
}

// ---------------------------------------------------------------------------
// Lane buckets and keys.
// ---------------------------------------------------------------------------

#[test]
fn lane_bucket_is_exhaustive_over_every_state() {
    use SidebarPrReviewLaneBucket::{Needs, Watching, Working};
    for (state, expected) in [
        (SidebarPrReviewState::Reviewing, Working),
        (SidebarPrReviewState::Submitting, Working),
        (SidebarPrReviewState::NeedsApproval, Needs),
        (SidebarPrReviewState::NeedsDecisionChanges, Needs),
        (SidebarPrReviewState::NeedsDecisionComment, Needs),
        (SidebarPrReviewState::NeedsDecision, Needs),
        (SidebarPrReviewState::HeadMoved, Needs),
        // A blocked monitor cannot make progress without the user.
        (SidebarPrReviewState::Blocked, Needs),
        (SidebarPrReviewState::Approved, Watching),
        (SidebarPrReviewState::ChangesRequested, Watching),
        (SidebarPrReviewState::Commented, Watching),
        (SidebarPrReviewState::Watching, Watching),
        // Pausing is a deliberate user choice, so it rests.
        (SidebarPrReviewState::Paused, Watching),
    ] {
        assert_eq!(state.lane_bucket(), expected, "state {state:?}");
    }
}

#[test]
fn keys_are_stable_snake_case_and_unique() {
    let keys: Vec<&str> = SidebarPrReviewState::ALL
        .iter()
        .map(|state| state.key())
        .collect();
    assert_eq!(
        keys,
        vec![
            "reviewing",
            "submitting",
            "needs_approval",
            "needs_decision_changes",
            "needs_decision_comment",
            "needs_decision",
            "head_moved",
            "blocked",
            "approved",
            "changes_requested",
            "commented",
            "watching",
            "paused",
        ]
    );
}

// ---------------------------------------------------------------------------
// Lifecycle gate parity (Step 4) — one case per SQL condition.
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_gate_admits_a_fully_eligible_review_pr_row() {
    let workspace = review_pr_workspace();
    let monitor = monitor(
        AgentWorkspacePrReviewMonitorStatus::Watching,
        Some("approve"),
    );
    assert!(lifecycle_monitor_for_sidebar(&workspace, &monitor).is_some());
}

#[test]
fn lifecycle_gate_rejects_a_non_review_pr_workspace_mode() {
    let mut workspace = review_pr_workspace();
    workspace.mode = AgentConversationWorkspaceMode::Edit;
    let monitor = monitor(
        AgentWorkspacePrReviewMonitorStatus::Watching,
        Some("approve"),
    );
    assert!(lifecycle_monitor_for_sidebar(&workspace, &monitor).is_none());
}

#[test]
fn lifecycle_gate_rejects_a_workspace_that_is_not_active() {
    for status in [
        AgentConversationWorkspaceStatus::Archived,
        AgentConversationWorkspaceStatus::Missing,
    ] {
        let mut workspace = review_pr_workspace();
        workspace.status = status;
        let monitor = monitor(
            AgentWorkspacePrReviewMonitorStatus::Watching,
            Some("approve"),
        );
        assert!(
            lifecycle_monitor_for_sidebar(&workspace, &monitor).is_none(),
            "status {status:?}"
        );
    }
}

#[test]
fn lifecycle_gate_rejects_terminal_publication_statuses() {
    for status in ["merged", "closed"] {
        let mut workspace = review_pr_workspace();
        workspace.publication_pr_status = Some(status.to_string());
        let monitor = monitor(
            AgentWorkspacePrReviewMonitorStatus::Watching,
            Some("approve"),
        );
        assert!(
            lifecycle_monitor_for_sidebar(&workspace, &monitor).is_none(),
            "publication status {status}"
        );
    }
}

#[test]
fn lifecycle_gate_admits_nonterminal_publication_statuses() {
    for status in [None, Some("open"), Some("draft")] {
        let mut workspace = review_pr_workspace();
        workspace.publication_pr_status = status.map(str::to_string);
        let monitor = monitor(
            AgentWorkspacePrReviewMonitorStatus::Watching,
            Some("approve"),
        );
        assert!(
            lifecycle_monitor_for_sidebar(&workspace, &monitor).is_some(),
            "publication status {status:?}"
        );
    }
}

#[test]
fn lifecycle_gate_rejects_a_terminal_monitor() {
    let workspace = review_pr_workspace();
    let monitor = monitor(
        AgentWorkspacePrReviewMonitorStatus::Terminal,
        Some("approve"),
    );
    assert!(lifecycle_monitor_for_sidebar(&workspace, &monitor).is_none());
}
