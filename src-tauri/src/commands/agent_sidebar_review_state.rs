//! Backend-owned Review PR state derivation for the agents sidebar.
//!
//! Every input comes from the PR review monitor plus workspace publication
//! status. Local `workspace_review_*` gate state is a different workflow and
//! must never leak in here — see `.claude/rules/agent-workspace-review-modes.md`.

use std::str::FromStr;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentRunStatus, AgentWorkspacePrReviewActionKind, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus,
};

/// Monitor-vocabulary outcomes that are not `AgentWorkspacePrReviewActionKind`
/// variants: the reviewer skipped the proposal, or the review produced no
/// action at all. Both rest rather than asking for a decision.
pub(crate) const PR_REVIEW_OUTCOME_SKIPPED: &str = "skipped";
pub(crate) const PR_REVIEW_OUTCOME_NO_ACTION: &str = "no_action";

/// The inbox lane a review state belongs to. Terminal rows never reach this
/// enum: the sidebar's merged/closed/archived check settles them first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarPrReviewLaneBucket {
    Needs,
    Working,
    Watching,
}

/// Review PR state as the sidebar presents it. `Watching` is the resting
/// classification this whole surface exists for: finished on your side, still
/// live on GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarPrReviewState {
    Reviewing,
    Submitting,
    NeedsApproval,
    NeedsDecisionChanges,
    NeedsDecisionComment,
    NeedsDecision,
    HeadMoved,
    Blocked,
    Approved,
    ChangesRequested,
    Commented,
    Watching,
    Paused,
}

impl SidebarPrReviewState {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 13] = [
        Self::Reviewing,
        Self::Submitting,
        Self::NeedsApproval,
        Self::NeedsDecisionChanges,
        Self::NeedsDecisionComment,
        Self::NeedsDecision,
        Self::HeadMoved,
        Self::Blocked,
        Self::Approved,
        Self::ChangesRequested,
        Self::Commented,
        Self::Watching,
        Self::Paused,
    ];

    /// Stable wire key. Frontend label/tone maps are keyed off these, so
    /// renaming one is a breaking change to the sidebar row meta line.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Reviewing => "reviewing",
            Self::Submitting => "submitting",
            Self::NeedsApproval => "needs_approval",
            Self::NeedsDecisionChanges => "needs_decision_changes",
            Self::NeedsDecisionComment => "needs_decision_comment",
            Self::NeedsDecision => "needs_decision",
            Self::HeadMoved => "head_moved",
            Self::Blocked => "blocked",
            Self::Approved => "approved",
            Self::ChangesRequested => "changes_requested",
            Self::Commented => "commented",
            Self::Watching => "watching",
            Self::Paused => "paused",
        }
    }

    pub(crate) fn lane_bucket(self) -> SidebarPrReviewLaneBucket {
        match self {
            Self::Reviewing | Self::Submitting => SidebarPrReviewLaneBucket::Working,
            // `Blocked` belongs here, not with the resting states: a blocked
            // monitor cannot make progress without the user.
            Self::NeedsApproval
            | Self::NeedsDecisionChanges
            | Self::NeedsDecisionComment
            | Self::NeedsDecision
            | Self::HeadMoved
            | Self::Blocked => SidebarPrReviewLaneBucket::Needs,
            // `Paused` rests because pausing is a deliberate user choice.
            Self::Approved
            | Self::ChangesRequested
            | Self::Commented
            | Self::Watching
            | Self::Paused => SidebarPrReviewLaneBucket::Watching,
        }
    }
}

/// Derives the sidebar's Review PR state for one row. `None` means "no review
/// classification applies" and the caller must fall back to the legacy lane
/// logic — the fail-closed default for rows with no monitor or a terminal one.
pub(crate) fn pr_review_state_for_row(
    monitor: Option<&AgentWorkspacePrReviewMonitor>,
    latest_run_status: Option<AgentRunStatus>,
) -> Option<SidebarPrReviewState> {
    let monitor = monitor?;
    if monitor.status == AgentWorkspacePrReviewMonitorStatus::Terminal {
        return None;
    }

    // A live run outranks a resting monitor status: the monitor row is only
    // updated at proposal/submission boundaries, so it lags an active reviewer.
    if matches!(latest_run_status, Some(AgentRunStatus::Running))
        || monitor.status == AgentWorkspacePrReviewMonitorStatus::Reviewing
    {
        return Some(SidebarPrReviewState::Reviewing);
    }

    let outcome = classify_review_outcome(monitor.last_review_outcome.as_deref());

    Some(match monitor.status {
        AgentWorkspacePrReviewMonitorStatus::Submitting => SidebarPrReviewState::Submitting,
        AgentWorkspacePrReviewMonitorStatus::AwaitingUser => {
            if head_moved_since_review(monitor) {
                SidebarPrReviewState::HeadMoved
            } else {
                match outcome {
                    ReviewOutcome::Action(AgentWorkspacePrReviewActionKind::Approve) => {
                        SidebarPrReviewState::NeedsApproval
                    }
                    ReviewOutcome::Action(AgentWorkspacePrReviewActionKind::RequestChanges) => {
                        SidebarPrReviewState::NeedsDecisionChanges
                    }
                    ReviewOutcome::Action(AgentWorkspacePrReviewActionKind::Comment) => {
                        SidebarPrReviewState::NeedsDecisionComment
                    }
                    ReviewOutcome::NoAction | ReviewOutcome::Unknown => {
                        SidebarPrReviewState::NeedsDecision
                    }
                }
            }
        }
        AgentWorkspacePrReviewMonitorStatus::Blocked => SidebarPrReviewState::Blocked,
        AgentWorkspacePrReviewMonitorStatus::Watching => match outcome {
            ReviewOutcome::Action(AgentWorkspacePrReviewActionKind::Approve) => {
                SidebarPrReviewState::Approved
            }
            ReviewOutcome::Action(AgentWorkspacePrReviewActionKind::RequestChanges) => {
                SidebarPrReviewState::ChangesRequested
            }
            ReviewOutcome::Action(AgentWorkspacePrReviewActionKind::Comment) => {
                SidebarPrReviewState::Commented
            }
            ReviewOutcome::NoAction | ReviewOutcome::Unknown => SidebarPrReviewState::Watching,
        },
        AgentWorkspacePrReviewMonitorStatus::Paused => SidebarPrReviewState::Paused,
        // A monitor exists but nothing has happened yet. This must read as
        // resting, not as needing the user.
        AgentWorkspacePrReviewMonitorStatus::Idle => SidebarPrReviewState::Watching,
        // Both settled above; repeated here so a new variant fails to compile.
        AgentWorkspacePrReviewMonitorStatus::Reviewing
        | AgentWorkspacePrReviewMonitorStatus::Terminal => SidebarPrReviewState::Reviewing,
    })
}

/// Applies the exact eligibility gate that `list_pr_review_lifecycle_monitors`
/// encodes in SQL, so the per-conversation read paths (mute fingerprint, bulk
/// publication poll) classify identically to the sidebar listing.
///
/// The workspace MUST be the raw domain entity. The response projection can
/// overlay a linked plan branch's publication status, while the SQL above reads
/// the raw `publication_pr_status` column; gating on the overlaid value would
/// admit or exclude a monitor the listing decided differently.
pub(crate) fn lifecycle_monitor_for_sidebar<'a>(
    workspace: &AgentConversationWorkspace,
    monitor: &'a AgentWorkspacePrReviewMonitor,
) -> Option<&'a AgentWorkspacePrReviewMonitor> {
    // `has_terminal_publication_pr_status` is the same predicate the memory
    // repository's listing uses for the SQL's `NOT IN ('merged', 'closed')`
    // clause, so parity holds by construction rather than by duplication.
    if workspace.mode != AgentConversationWorkspaceMode::ReviewPr
        || workspace.status != AgentConversationWorkspaceStatus::Active
        || workspace.has_terminal_publication_pr_status()
        || monitor.status == AgentWorkspacePrReviewMonitorStatus::Terminal
    {
        return None;
    }

    Some(monitor)
}

/// The three shapes `last_review_outcome` can take. `NoAction` and `Unknown`
/// both rest, but keeping them apart documents which strings are known monitor
/// vocabulary and which are unrecognized.
enum ReviewOutcome {
    Action(AgentWorkspacePrReviewActionKind),
    NoAction,
    Unknown,
}

fn classify_review_outcome(outcome: Option<&str>) -> ReviewOutcome {
    match outcome {
        Some(PR_REVIEW_OUTCOME_SKIPPED | PR_REVIEW_OUTCOME_NO_ACTION) => ReviewOutcome::NoAction,
        Some(outcome) => AgentWorkspacePrReviewActionKind::from_str(outcome)
            .map_or(ReviewOutcome::Unknown, ReviewOutcome::Action),
        None => ReviewOutcome::Unknown,
    }
}

fn head_moved_since_review(monitor: &AgentWorkspacePrReviewMonitor) -> bool {
    match (
        monitor.last_reviewed_head_sha.as_deref(),
        monitor.last_seen_head_sha.as_deref(),
    ) {
        (Some(reviewed), Some(seen)) => reviewed != seen,
        // One side unknown is not evidence the head moved.
        _ => false,
    }
}
