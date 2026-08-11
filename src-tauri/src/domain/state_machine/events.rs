// Task events - triggers for state machine transitions
// These events come from user actions, agent signals, or system signals

use serde::{Deserialize, Serialize};

/// All possible events that can trigger state transitions in the task state machine.
///
/// Events are categorized into:
/// - User actions: Manual interventions by humans
/// - Agent signals: Completion/failure signals from AI agents
/// - System signals: Automatic triggers based on system state
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskEvent {
    // ==================
    // User actions
    // ==================
    /// User moves task from Backlog to Ready
    Schedule,

    /// User explicitly starts execution (Ready → Executing)
    /// Used when dragging task to "In Progress" column in Kanban
    StartExecution,

    /// System starts AI review (PendingReview → Reviewing)
    StartReview,

    /// System starts revision work (RevisionNeeded → ReExecuting)
    StartRevision,

    /// User cancels task from any non-terminal state
    Cancel,

    /// User pauses a running task (can resume later)
    Pause,

    /// User stops a running task (terminal, requires manual restart)
    Stop,

    /// Human override to approve task regardless of review state
    ForceApprove,

    /// Human approves task after AI review (ReviewPassed → Approved)
    HumanApprove,

    /// Human requests changes after AI review (ReviewPassed → RevisionNeeded)
    HumanRequestChanges {
        /// Feedback from human reviewer
        feedback: String,
    },

    /// Retry task from Failed, Cancelled, or Approved state
    Retry,

    /// Human skips QA failure and proceeds to review
    SkipQa,

    // ==================
    // Agent signals
    // ==================
    /// Worker agent completed execution successfully
    ExecutionComplete,

    /// Worker agent execution failed with error
    ExecutionFailed {
        /// The error message from the agent
        error: String,
    },

    /// Agent needs human input to proceed
    NeedsHumanInput {
        /// Reason why human input is needed
        reason: String,
    },

    /// QA prep agent completed acceptance criteria refinement
    QaRefinementComplete,

    /// QA testing completed with result
    QaTestsComplete {
        /// Whether all tests passed
        passed: bool,
    },

    /// AI reviewer completed review
    ReviewComplete {
        /// Whether the review approved the work
        approved: bool,
        /// Optional feedback from reviewer
        feedback: Option<String>,
    },

    // ==================
    // Merge events
    // ==================
    /// System starts merge workflow (Approved → PendingMerge)
    StartMerge,

    /// Programmatic merge completed successfully (PendingMerge → Merged)
    MergeComplete,

    /// Programmatic merge detected conflicts, needs agent (PendingMerge → Merging)
    MergeConflict,

    /// Merge agent failed to resolve conflicts (Merging → MergeConflict)
    MergeAgentFailed,

    /// Merge failed due to non-conflict errors (Merging → MergeIncomplete)
    MergeAgentError,

    /// User manually resolved merge conflicts (MergeConflict → Merged)
    ConflictResolved,

    // ==================
    // System signals
    // ==================
    /// All blocking tasks have been completed
    BlockersResolved,

    /// A blocking dependency was detected
    BlockerDetected {
        /// The ID of the blocking task
        blocker_id: String,
    },

    /// Legacy freshness signal retained for persisted/event compatibility.
    /// New workflow code must use a direction-specific branch-update event.
    BranchFreshnessConflict,
    /// A durable plan-branch synchronization checkpoint was activated.
    PlanBranchUpdateRequired,
    /// A durable task-branch synchronization checkpoint was activated.
    TaskBranchUpdateRequired,
}

impl TaskEvent {
    /// Returns true if this is a user-initiated action
    pub fn is_user_action(&self) -> bool {
        matches!(
            self,
            TaskEvent::Schedule
                | TaskEvent::StartExecution
                | TaskEvent::Cancel
                | TaskEvent::Pause
                | TaskEvent::Stop
                | TaskEvent::ForceApprove
                | TaskEvent::HumanApprove
                | TaskEvent::HumanRequestChanges { .. }
                | TaskEvent::Retry
                | TaskEvent::SkipQa
                | TaskEvent::ConflictResolved
        )
    }

    /// Returns true if this is an agent-originated signal
    pub fn is_agent_signal(&self) -> bool {
        matches!(
            self,
            TaskEvent::ExecutionComplete
                | TaskEvent::ExecutionFailed { .. }
                | TaskEvent::NeedsHumanInput { .. }
                | TaskEvent::QaRefinementComplete
                | TaskEvent::QaTestsComplete { .. }
                | TaskEvent::ReviewComplete { .. }
                | TaskEvent::MergeAgentFailed
                | TaskEvent::MergeAgentError
        )
    }

    /// Returns true if this is a system-generated signal
    pub fn is_system_signal(&self) -> bool {
        matches!(
            self,
            TaskEvent::BlockersResolved
                | TaskEvent::BlockerDetected { .. }
                | TaskEvent::StartReview
                | TaskEvent::StartRevision
                | TaskEvent::StartMerge
                | TaskEvent::MergeComplete
                | TaskEvent::MergeConflict
                | TaskEvent::BranchFreshnessConflict
                | TaskEvent::PlanBranchUpdateRequired
                | TaskEvent::TaskBranchUpdateRequired
        )
    }

    /// Returns true if this is a merge-related event
    pub fn is_merge_event(&self) -> bool {
        matches!(
            self,
            TaskEvent::StartMerge
                | TaskEvent::MergeComplete
                | TaskEvent::MergeConflict
                | TaskEvent::MergeAgentFailed
                | TaskEvent::MergeAgentError
                | TaskEvent::ConflictResolved
        )
    }

    /// Returns the event name as a string for logging
    pub fn name(&self) -> &'static str {
        match self {
            TaskEvent::Schedule => "Schedule",
            TaskEvent::StartExecution => "StartExecution",
            TaskEvent::StartReview => "StartReview",
            TaskEvent::StartRevision => "StartRevision",
            TaskEvent::Cancel => "Cancel",
            TaskEvent::Pause => "Pause",
            TaskEvent::Stop => "Stop",
            TaskEvent::ForceApprove => "ForceApprove",
            TaskEvent::HumanApprove => "HumanApprove",
            TaskEvent::HumanRequestChanges { .. } => "HumanRequestChanges",
            TaskEvent::Retry => "Retry",
            TaskEvent::SkipQa => "SkipQa",
            TaskEvent::ExecutionComplete => "ExecutionComplete",
            TaskEvent::ExecutionFailed { .. } => "ExecutionFailed",
            TaskEvent::NeedsHumanInput { .. } => "NeedsHumanInput",
            TaskEvent::QaRefinementComplete => "QaRefinementComplete",
            TaskEvent::QaTestsComplete { .. } => "QaTestsComplete",
            TaskEvent::ReviewComplete { .. } => "ReviewComplete",
            TaskEvent::StartMerge => "StartMerge",
            TaskEvent::MergeComplete => "MergeComplete",
            TaskEvent::MergeConflict => "MergeConflict",
            TaskEvent::MergeAgentFailed => "MergeAgentFailed",
            TaskEvent::MergeAgentError => "MergeAgentError",
            TaskEvent::ConflictResolved => "ConflictResolved",
            TaskEvent::BlockersResolved => "BlockersResolved",
            TaskEvent::BlockerDetected { .. } => "BlockerDetected",
            TaskEvent::BranchFreshnessConflict => "BranchFreshnessConflict",
            TaskEvent::PlanBranchUpdateRequired => "PlanBranchUpdateRequired",
            TaskEvent::TaskBranchUpdateRequired => "TaskBranchUpdateRequired",
        }
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
