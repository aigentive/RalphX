// Core types and dispatcher for TaskStateMachine

use super::super::context::TaskContext;
use super::super::events::TaskEvent;
use super::super::types::{FailedData, QaFailedData};
use std::fmt;
use std::str::FromStr;
use tracing::{debug, info};

/// The task state machine shared data (context)
#[derive(Debug)]
pub struct TaskStateMachine {
    pub context: TaskContext,
}

/// All possible states for a task
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    // Idle states
    Backlog,
    Ready,
    Blocked,

    // Execution states
    Executing,
    ReExecuting,

    // QA states
    QaRefining,
    QaTesting,
    QaPassed,
    QaFailed(QaFailedData),

    // Review states
    PendingReview,
    Reviewing,
    ReviewPassed,
    Escalated,
    RevisionNeeded,

    // Approval (leads to merge workflow)
    Approved,

    // Merge states
    PendingMerge,
    Merging,
    WaitingOnPr,
    MergeIncomplete,
    MergeConflict,
    Merged,

    // Dedicated branch synchronization states
    UpdatingPlanBranch,
    UpdatingTaskBranch,
    BranchUpdateBlocked,

    // Terminal states
    Failed(FailedData),
    Cancelled,
    Stopped, // Stopped by user, requires manual restart

    // Suspended states (not terminal)
    Paused, // Can resume to previous agent-active state
}

impl State {
    /// Returns true if this is a terminal state
    /// Terminal: Merged, Failed, Cancelled, Stopped
    /// NOT terminal: Paused (can resume to previous state)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            State::Merged | State::Failed(_) | State::Cancelled | State::Stopped
        )
    }

    /// Returns true if this is an idle state
    pub fn is_idle(&self) -> bool {
        matches!(self, State::Backlog | State::Ready | State::Blocked)
    }

    /// Returns true if this is an active (non-idle, non-terminal, non-paused) state
    pub fn is_active(&self) -> bool {
        !self.is_idle() && !self.is_terminal() && !self.is_paused()
    }

    /// Returns true if this is the paused state
    pub fn is_paused(&self) -> bool {
        matches!(self, State::Paused)
    }

    /// Returns true if this is a merge state
    pub fn is_merge(&self) -> bool {
        matches!(
            self,
            State::PendingMerge
                | State::Merging
                | State::WaitingOnPr
                | State::MergeIncomplete
                | State::MergeConflict
                | State::Merged
        )
    }
}

/// Response type for state machine transitions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// Transition was handled
    Handled,
    /// Event was not applicable in current state
    NotHandled,
    /// Transition to a new state
    Transition(State),
}

impl TaskStateMachine {
    pub fn new(context: TaskContext) -> Self {
        Self { context }
    }

    /// Dispatch an event to the current state
    ///
    /// This is the main entry point for processing events. It logs the dispatch
    /// at debug level, routes the event to the appropriate state handler, and
    /// logs any resulting transition at info level.
    pub fn dispatch(&mut self, state: &State, event: &TaskEvent) -> Response {
        // Log the dispatch at debug level
        self.on_dispatch(state, event);

        let response = match state {
            State::Backlog => self.backlog(event),
            State::Ready => self.ready(event),
            State::Blocked => self.blocked(event),
            State::Executing => self.executing(event),
            State::ReExecuting => self.re_executing(event),
            State::QaRefining => self.qa_refining(event),
            State::QaTesting => self.qa_testing(event),
            State::QaPassed => self.qa_passed(event),
            State::QaFailed(data) => self.qa_failed(event, data),
            State::PendingReview => self.pending_review(event),
            State::Reviewing => self.reviewing(event),
            State::ReviewPassed => self.review_passed(event),
            State::Escalated => self.escalated(event),
            State::RevisionNeeded => self.revision_needed(event),
            State::Approved => self.approved(event),
            State::PendingMerge => self.pending_merge(event),
            State::Merging => self.merging(event),
            State::UpdatingPlanBranch | State::UpdatingTaskBranch => self.branch_updating(event),
            State::BranchUpdateBlocked => self.branch_update_blocked(event),
            State::WaitingOnPr => self.waiting_on_pr(event),
            State::MergeIncomplete => self.merge_incomplete(event),
            State::MergeConflict => self.merge_conflict(event),
            State::Merged => self.merged(event),
            State::Failed(data) => self.failed(event, data),
            State::Cancelled => self.cancelled(event),
            State::Stopped => self.stopped(event),
            State::Paused => self.paused(event),
        };

        // Log transition at info level if a transition occurred
        if let Response::Transition(ref new_state) = response {
            self.on_transition(state, new_state, event);
        }

        response
    }

    /// Hook called before dispatching an event to a state handler.
    ///
    /// Logs the state/event pair at debug level for detailed tracing.
    fn on_dispatch(&self, state: &State, event: &TaskEvent) {
        debug!(
            task_id = %self.context.task_id,
            project_id = %self.context.project_id,
            state = %state.name(),
            event = %event.name(),
            "Dispatching event to state"
        );
    }

    /// Hook called after a state transition occurs.
    ///
    /// Logs the from/to states at info level for visibility into state changes.
    fn on_transition(&self, from: &State, to: &State, event: &TaskEvent) {
        info!(
            task_id = %self.context.task_id,
            project_id = %self.context.project_id,
            from_state = %from.name(),
            to_state = %to.name(),
            event = %event.name(),
            "State transition"
        );
    }
}

impl State {
    /// Returns a human-readable name for the state (for logging)
    pub fn name(&self) -> &'static str {
        match self {
            State::Backlog => "Backlog",
            State::Ready => "Ready",
            State::Blocked => "Blocked",
            State::Executing => "Executing",
            State::ReExecuting => "ReExecuting",
            State::QaRefining => "QaRefining",
            State::QaTesting => "QaTesting",
            State::QaPassed => "QaPassed",
            State::QaFailed(_) => "QaFailed",
            State::PendingReview => "PendingReview",
            State::Reviewing => "Reviewing",
            State::ReviewPassed => "ReviewPassed",
            State::Escalated => "Escalated",
            State::RevisionNeeded => "RevisionNeeded",
            State::Approved => "Approved",
            State::PendingMerge => "PendingMerge",
            State::Merging => "Merging",
            State::WaitingOnPr => "WaitingOnPr",
            State::MergeIncomplete => "MergeIncomplete",
            State::MergeConflict => "MergeConflict",
            State::Merged => "Merged",
            State::UpdatingPlanBranch => "UpdatingPlanBranch",
            State::UpdatingTaskBranch => "UpdatingTaskBranch",
            State::BranchUpdateBlocked => "BranchUpdateBlocked",
            State::Failed(_) => "Failed",
            State::Cancelled => "Cancelled",
            State::Stopped => "Stopped",
            State::Paused => "Paused",
        }
    }

    /// Returns the snake_case string representation for SQLite storage.
    ///
    /// This matches the InternalStatus as_str() format for consistency
    /// with the tasks table internal_status column.
    pub fn as_str(&self) -> &'static str {
        match self {
            State::Backlog => "backlog",
            State::Ready => "ready",
            State::Blocked => "blocked",
            State::Executing => "executing",
            State::ReExecuting => "re_executing",
            State::QaRefining => "qa_refining",
            State::QaTesting => "qa_testing",
            State::QaPassed => "qa_passed",
            State::QaFailed(_) => "qa_failed",
            State::PendingReview => "pending_review",
            State::Reviewing => "reviewing",
            State::ReviewPassed => "review_passed",
            State::Escalated => "escalated",
            State::RevisionNeeded => "revision_needed",
            State::Approved => "approved",
            State::PendingMerge => "pending_merge",
            State::Merging => "merging",
            State::WaitingOnPr => "waiting_on_pr",
            State::MergeIncomplete => "merge_incomplete",
            State::MergeConflict => "merge_conflict",
            State::Merged => "merged",
            State::UpdatingPlanBranch => "updating_plan_branch",
            State::UpdatingTaskBranch => "updating_task_branch",
            State::BranchUpdateBlocked => "branch_update_blocked",
            State::Failed(_) => "failed",
            State::Cancelled => "cancelled",
            State::Stopped => "stopped",
            State::Paused => "paused",
        }
    }
}

/// Error returned when parsing a State from a string fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseStateError {
    pub invalid_value: String,
}

impl fmt::Display for ParseStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid state: '{}'", self.invalid_value)
    }
}

impl std::error::Error for ParseStateError {}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for State {
    type Err = ParseStateError;

    /// Parses a snake_case string into a State.
    ///
    /// For states with local data (QaFailed, Failed), this returns the variant
    /// with default data. To restore actual data, use the persistence helpers.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "backlog" => Ok(State::Backlog),
            "ready" => Ok(State::Ready),
            "blocked" => Ok(State::Blocked),
            "executing" => Ok(State::Executing),
            "re_executing" => Ok(State::ReExecuting),
            "qa_refining" => Ok(State::QaRefining),
            "qa_testing" => Ok(State::QaTesting),
            "qa_passed" => Ok(State::QaPassed),
            "qa_failed" => Ok(State::QaFailed(QaFailedData::default())),
            "pending_review" => Ok(State::PendingReview),
            "reviewing" => Ok(State::Reviewing),
            "review_passed" => Ok(State::ReviewPassed),
            "escalated" => Ok(State::Escalated),
            "revision_needed" => Ok(State::RevisionNeeded),
            "approved" => Ok(State::Approved),
            "pending_merge" => Ok(State::PendingMerge),
            "merging" => Ok(State::Merging),
            "waiting_on_pr" => Ok(State::WaitingOnPr),
            "merge_incomplete" => Ok(State::MergeIncomplete),
            "merge_conflict" => Ok(State::MergeConflict),
            "merged" => Ok(State::Merged),
            "updating_plan_branch" => Ok(State::UpdatingPlanBranch),
            "updating_task_branch" => Ok(State::UpdatingTaskBranch),
            "branch_update_blocked" => Ok(State::BranchUpdateBlocked),
            "failed" => Ok(State::Failed(FailedData::default())),
            "cancelled" => Ok(State::Cancelled),
            "stopped" => Ok(State::Stopped),
            "paused" => Ok(State::Paused),
            _ => Err(ParseStateError {
                invalid_value: s.to_string(),
            }),
        }
    }
}
