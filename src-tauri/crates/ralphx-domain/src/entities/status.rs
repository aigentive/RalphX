// InternalStatus enum representing the 14 internal states of a task
// with transition validation for the state machine

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// The internal statuses that a task can be in.
/// These map to external Kanban columns via WorkflowSchema (Phase 11).
///
/// State machine transitions are validated - not all transitions are allowed.
/// Use `can_transition_to()` to check if a transition is valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InternalStatus {
    /// Task is in the backlog, not yet ready for execution
    Backlog,
    /// Task is ready to be picked up for execution
    Ready,
    /// Task is blocked by dependencies or external factors
    Blocked,
    /// Task is currently being executed by an agent
    Executing,
    /// QA is refining test criteria based on implementation
    QaRefining,
    /// QA tests are being executed
    QaTesting,
    /// QA tests have passed
    QaPassed,
    /// QA tests have failed, needs revision
    QaFailed,
    /// Awaiting code review (AI or human)
    PendingReview,
    /// AI agent is actively reviewing
    Reviewing,
    /// AI approved, awaiting human confirmation
    ReviewPassed,
    /// AI couldn't decide, escalated to human for decision
    Escalated,
    /// Reviewer requested changes
    RevisionNeeded,
    /// Worker is revising based on review feedback
    ReExecuting,
    /// Task has been approved and is complete
    Approved,
    /// Approved, awaiting merge (auto-transition from Approved)
    PendingMerge,
    /// Merge agent attempting to resolve conflicts
    Merging,
    /// Waiting for an external pull request review/merge in PR mode
    WaitingOnPr,
    /// Merge failed due to non-conflict errors (agent/git operation failure)
    MergeIncomplete,
    /// Merge failed, needs manual resolution
    MergeConflict,
    /// Successfully merged to base branch
    Merged,
    /// Task has permanently failed after max retries
    Failed,
    /// Task was cancelled by user
    Cancelled,
    /// Task was paused (non-terminal, can resume to previous state)
    Paused,
    /// Task was stopped (terminal, requires manual restart)
    Stopped,
}

impl InternalStatus {
    /// Returns the list of valid statuses this status can transition to.
    /// This enforces the state machine rules from the master plan.
    pub fn valid_transitions(&self) -> &'static [InternalStatus] {
        use InternalStatus::*;
        match self {
            // Idle states
            Backlog => &[Ready, Cancelled],
            Ready => &[Executing, Blocked, PendingMerge, Cancelled],
            Blocked => &[Ready, Cancelled],

            // Execution states
            Executing => &[QaRefining, PendingReview, Failed, Blocked, Stopped, Paused],

            // QA states
            QaRefining => &[QaTesting, Stopped, Paused],
            QaTesting => &[QaPassed, QaFailed, Stopped, Paused],
            QaPassed => &[PendingReview],
            QaFailed => &[RevisionNeeded],

            // Review states
            PendingReview => &[Reviewing],
            Reviewing => &[ReviewPassed, RevisionNeeded, Escalated, Stopped, Paused],
            ReviewPassed => &[Approved, RevisionNeeded],
            Escalated => &[Approved, RevisionNeeded, PendingReview],
            RevisionNeeded => &[ReExecuting, Cancelled],
            ReExecuting => &[PendingReview, Failed, Blocked, Stopped, Paused],

            // Approval leads to merge workflow
            Approved => &[PendingMerge, Ready],

            // Merge states
            PendingMerge => &[
                Merged,
                Merging,
                WaitingOnPr,
                MergeIncomplete,
                Stopped,
                Paused,
                Cancelled,
            ], // Success → Merged, Conflict → Merging (agent), PR mode → WaitingOnPr, Error → MergeIncomplete
            Merging => &[
                Merged,
                WaitingOnPr,
                MergeConflict,
                MergeIncomplete,
                Stopped,
                Paused,
                Cancelled,
            ], // Agent success → Merged, Agent failure → MergeConflict, Non-conflict error → MergeIncomplete
            WaitingOnPr => &[
                Merged,
                MergeIncomplete,
                PendingMerge,
                Stopped,
                Paused,
                Cancelled,
            ], // PR merged → Merged, closed/error → MergeIncomplete, retry → PendingMerge
            MergeIncomplete => &[
                PendingMerge,
                Merging,
                WaitingOnPr,
                Merged,
                Stopped,
                Paused,
                Cancelled,
            ], // Retry → PendingMerge, agent spawn → Merging, PR repair → WaitingOnPr, manual resolution → Merged
            MergeConflict => &[PendingMerge, Merging, Merged, Stopped, Paused, Cancelled], // Retry → PendingMerge, Agent spawn → Merging, Manual resolution → Merged

            // Terminal states (can be re-opened)
            Merged => &[Ready],
            Failed => &[Ready],
            Cancelled => &[Ready],
            Stopped => &[Ready], // Terminal: requires manual restart

            // Paused: can resume to previous agent-active state
            // Resume uses status history to restore to the pre-pause state
            Paused => &[
                Executing,
                ReExecuting,
                QaRefining,
                QaTesting,
                Reviewing,
                Merging,
                WaitingOnPr,
            ],
        }
    }

    /// Checks if transitioning to the target status is valid.
    pub fn can_transition_to(&self, target: InternalStatus) -> bool {
        self.valid_transitions().contains(&target)
    }

    /// Returns all possible InternalStatus variants.
    /// Useful for iteration and testing.
    pub fn all_variants() -> &'static [InternalStatus] {
        use InternalStatus::*;
        &[
            Backlog,
            Ready,
            Blocked,
            Executing,
            QaRefining,
            QaTesting,
            QaPassed,
            QaFailed,
            PendingReview,
            Reviewing,
            ReviewPassed,
            Escalated,
            RevisionNeeded,
            ReExecuting,
            Approved,
            PendingMerge,
            Merging,
            WaitingOnPr,
            MergeIncomplete,
            MergeConflict,
            Merged,
            Failed,
            Cancelled,
            Paused,
            Stopped,
        ]
    }

    /// Returns true if this status is terminal (no automatic progress possible).
    /// Terminal: Merged, Failed, Cancelled, Stopped, MergeIncomplete.
    /// Note: terminal != dependency-satisfied. Failed and Stopped are terminal
    /// but do NOT satisfy dependencies (see `is_dependency_satisfied()`).
    /// NOT terminal: Paused (can resume), MergeConflict (agent can retry), Approved (→ PendingMerge).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Merged | Self::Failed | Self::Cancelled | Self::Stopped | Self::MergeIncomplete
        )
    }

    /// Whether this status satisfies a dependency (unblocks dependents).
    /// Only Merged and Cancelled satisfy — the task completed its purpose.
    /// Failed is NOT satisfied — dependents stay Blocked to prevent cascade
    /// execution against broken output. Users must manually unblock or cancel.
    /// Stopped is NOT satisfied — the task was interrupted before completing.
    /// MergeIncomplete is NOT satisfied — the merge failed (conflict, validation
    /// failure), code is not on the target branch, dependents should not proceed.
    /// Contrast with `is_terminal()` which also includes Failed, Stopped, MergeIncomplete.
    pub fn is_dependency_satisfied(&self) -> bool {
        matches!(self, Self::Merged | Self::Cancelled)
    }

    /// Whether this status still actively blocks dependent tasks.
    pub fn is_active_dependency_blocker(&self) -> bool {
        !self.is_dependency_satisfied()
    }

    /// Returns the snake_case string representation (matches serde serialization)
    pub fn as_str(&self) -> &'static str {
        match self {
            InternalStatus::Backlog => "backlog",
            InternalStatus::Ready => "ready",
            InternalStatus::Blocked => "blocked",
            InternalStatus::Executing => "executing",
            InternalStatus::QaRefining => "qa_refining",
            InternalStatus::QaTesting => "qa_testing",
            InternalStatus::QaPassed => "qa_passed",
            InternalStatus::QaFailed => "qa_failed",
            InternalStatus::PendingReview => "pending_review",
            InternalStatus::Reviewing => "reviewing",
            InternalStatus::ReviewPassed => "review_passed",
            InternalStatus::Escalated => "escalated",
            InternalStatus::RevisionNeeded => "revision_needed",
            InternalStatus::ReExecuting => "re_executing",
            InternalStatus::Approved => "approved",
            InternalStatus::PendingMerge => "pending_merge",
            InternalStatus::Merging => "merging",
            InternalStatus::WaitingOnPr => "waiting_on_pr",
            InternalStatus::MergeIncomplete => "merge_incomplete",
            InternalStatus::MergeConflict => "merge_conflict",
            InternalStatus::Merged => "merged",
            InternalStatus::Failed => "failed",
            InternalStatus::Cancelled => "cancelled",
            InternalStatus::Paused => "paused",
            InternalStatus::Stopped => "stopped",
        }
    }
}

impl fmt::Display for InternalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Error type for parsing InternalStatus from a string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseInternalStatusError {
    pub value: String,
}

impl fmt::Display for ParseInternalStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown internal status: '{}'", self.value)
    }
}

impl std::error::Error for ParseInternalStatusError {}

impl FromStr for InternalStatus {
    type Err = ParseInternalStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "backlog" => Ok(InternalStatus::Backlog),
            "ready" => Ok(InternalStatus::Ready),
            "blocked" => Ok(InternalStatus::Blocked),
            "executing" => Ok(InternalStatus::Executing),
            "qa_refining" => Ok(InternalStatus::QaRefining),
            "qa_testing" => Ok(InternalStatus::QaTesting),
            "qa_passed" => Ok(InternalStatus::QaPassed),
            "qa_failed" => Ok(InternalStatus::QaFailed),
            "pending_review" => Ok(InternalStatus::PendingReview),
            "reviewing" => Ok(InternalStatus::Reviewing),
            "review_passed" => Ok(InternalStatus::ReviewPassed),
            "escalated" => Ok(InternalStatus::Escalated),
            "revision_needed" => Ok(InternalStatus::RevisionNeeded),
            "re_executing" => Ok(InternalStatus::ReExecuting),
            "approved" => Ok(InternalStatus::Approved),
            "pending_merge" => Ok(InternalStatus::PendingMerge),
            "merging" => Ok(InternalStatus::Merging),
            "waiting_on_pr" => Ok(InternalStatus::WaitingOnPr),
            "merge_incomplete" => Ok(InternalStatus::MergeIncomplete),
            "merge_conflict" => Ok(InternalStatus::MergeConflict),
            "merged" => Ok(InternalStatus::Merged),
            "failed" => Ok(InternalStatus::Failed),
            "cancelled" => Ok(InternalStatus::Cancelled),
            "paused" => Ok(InternalStatus::Paused),
            "stopped" => Ok(InternalStatus::Stopped),
            _ => Err(ParseInternalStatusError {
                value: s.to_string(),
            }),
        }
    }
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
