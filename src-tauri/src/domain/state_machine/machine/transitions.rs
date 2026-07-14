// State transition handlers for TaskStateMachine
// This module contains all the state-specific event handlers

use super::super::events::TaskEvent;
use super::super::types::{Blocker, FailedData, QaFailedData};
use crate::domain::state_machine::machine::types::{Response, State, TaskStateMachine};

impl TaskStateMachine {
    // ==================
    // Idle States
    // ==================

    /// Backlog state - task is parked, not ready for work
    pub fn backlog(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::Schedule => Response::Transition(State::Ready),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// Ready state - task is ready to be picked up
    pub fn ready(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::StartExecution => {
                // User explicitly starts execution (e.g., drag to "In Progress")
                Response::Transition(State::Executing)
            }
            TaskEvent::BlockerDetected { blocker_id } => {
                // Add blocker to context
                self.context.add_blocker(Blocker::new(blocker_id.clone()));
                Response::Transition(State::Blocked)
            }
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// Blocked state - waiting on dependencies or human input
    pub fn blocked(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::BlockersResolved => {
                self.context.resolve_all_blockers();
                Response::Transition(State::Ready)
            }
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    // ==================
    // Execution States
    // ==================

    /// Executing state - worker agent is actively running (first attempt)
    pub fn executing(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::ExecutionComplete => {
                // Check qa_enabled directly instead of going to ExecutionDone
                if self.context.qa_enabled {
                    Response::Transition(State::QaRefining)
                } else {
                    Response::Transition(State::PendingReview)
                }
            }
            TaskEvent::ExecutionFailed { error } => {
                Response::Transition(State::Failed(FailedData::new(error.clone())))
            }
            TaskEvent::NeedsHumanInput { reason } => {
                self.context
                    .add_blocker(Blocker::human_input(reason.clone()));
                Response::Transition(State::Blocked)
            }
            TaskEvent::PlanBranchUpdateRequired => Response::Transition(State::UpdatingPlanBranch),
            TaskEvent::TaskBranchUpdateRequired => Response::Transition(State::UpdatingTaskBranch),
            TaskEvent::Pause => Response::Transition(State::Paused),
            TaskEvent::Stop => Response::Transition(State::Stopped),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// ReExecuting state - worker revising after failed review
    pub fn re_executing(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::ExecutionComplete => {
                // Check qa_enabled to decide next step
                if self.context.qa_enabled {
                    Response::Transition(State::QaRefining)
                } else {
                    Response::Transition(State::PendingReview)
                }
            }
            TaskEvent::ExecutionFailed { error } => {
                Response::Transition(State::Failed(FailedData::new(error.clone())))
            }
            TaskEvent::BlockerDetected { blocker_id: _ } => Response::Transition(State::Blocked),
            TaskEvent::PlanBranchUpdateRequired => Response::Transition(State::UpdatingPlanBranch),
            TaskEvent::TaskBranchUpdateRequired => Response::Transition(State::UpdatingTaskBranch),
            TaskEvent::Pause => Response::Transition(State::Paused),
            TaskEvent::Stop => Response::Transition(State::Stopped),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    // ==================
    // QA States
    // ==================

    /// QaRefining state - QA agent refining test plan based on actual changes
    pub fn qa_refining(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::QaRefinementComplete => Response::Transition(State::QaTesting),
            TaskEvent::ExecutionFailed { error } => {
                Response::Transition(State::Failed(FailedData::new(error.clone())))
            }
            TaskEvent::Pause => Response::Transition(State::Paused),
            TaskEvent::Stop => Response::Transition(State::Stopped),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// QaTesting state - QA tests are executing
    pub fn qa_testing(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::QaTestsComplete { passed: true } => Response::Transition(State::QaPassed),
            TaskEvent::QaTestsComplete { passed: false } => {
                Response::Transition(State::QaFailed(QaFailedData::default()))
            }
            TaskEvent::Pause => Response::Transition(State::Paused),
            TaskEvent::Stop => Response::Transition(State::Stopped),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// QaPassed state - all QA tests passed
    pub fn qa_passed(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            // Auto-transition to PendingReview happens via entry action
            _ => Response::NotHandled,
        }
    }

    /// QaFailed state - QA tests failed
    pub fn qa_failed(&mut self, event: &TaskEvent, _data: &QaFailedData) -> Response {
        match event {
            TaskEvent::Retry => Response::Transition(State::RevisionNeeded),
            TaskEvent::SkipQa => Response::Transition(State::PendingReview),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    // ==================
    // Review States
    // ==================

    /// PendingReview state - awaiting AI reviewer to pick up
    pub fn pending_review(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            // Transition to Reviewing happens via entry action (spawns reviewer)
            _ => Response::NotHandled,
        }
    }

    /// Reviewing state - AI agent is actively reviewing
    pub fn reviewing(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::ReviewComplete {
                approved: true,
                feedback,
            } => {
                if let Some(fb) = feedback {
                    self.context.review_feedback = Some(fb.clone());
                }
                Response::Transition(State::ReviewPassed)
            }
            TaskEvent::ReviewComplete {
                approved: false,
                feedback,
            } => {
                if let Some(fb) = feedback {
                    self.context.review_feedback = Some(fb.clone());
                }
                Response::Transition(State::RevisionNeeded)
            }
            TaskEvent::PlanBranchUpdateRequired => Response::Transition(State::UpdatingPlanBranch),
            TaskEvent::TaskBranchUpdateRequired => Response::Transition(State::UpdatingTaskBranch),
            TaskEvent::Pause => Response::Transition(State::Paused),
            TaskEvent::Stop => Response::Transition(State::Stopped),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// ReviewPassed state - AI approved, awaiting human confirmation
    pub fn review_passed(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::HumanApprove => Response::Transition(State::Approved),
            TaskEvent::HumanRequestChanges { feedback } => {
                self.context.review_feedback = Some(feedback.clone());
                Response::Transition(State::RevisionNeeded)
            }
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// Escalated state - AI couldn't decide, awaiting human decision
    pub fn escalated(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::HumanApprove => Response::Transition(State::Approved),
            TaskEvent::HumanRequestChanges { feedback } => {
                self.context.review_feedback = Some(feedback.clone());
                Response::Transition(State::RevisionNeeded)
            }
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// RevisionNeeded state - review found issues, ready for re-execution
    pub fn revision_needed(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            // Auto-transition to ReExecuting happens via entry action
            _ => Response::NotHandled,
        }
    }

    // ==================
    // Approval and Merge States
    // ==================

    /// Approved state - complete and verified, leads to merge workflow
    pub fn approved(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::StartMerge => Response::Transition(State::PendingMerge),
            TaskEvent::Retry => {
                self.context.clear_review_feedback();
                Response::Transition(State::Ready)
            }
            _ => Response::NotHandled,
        }
    }

    /// PendingMerge state - awaiting programmatic merge attempt
    pub fn pending_merge(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::PlanBranchUpdateRequired => Response::Transition(State::UpdatingPlanBranch),
            TaskEvent::TaskBranchUpdateRequired => Response::Transition(State::UpdatingTaskBranch),
            TaskEvent::MergeComplete => Response::Transition(State::Merged),
            TaskEvent::MergeConflict => Response::Transition(State::Merging),
            TaskEvent::Pause => Response::Transition(State::Paused),
            TaskEvent::Stop => Response::Transition(State::Stopped),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// WaitingOnPr state - external pull request is open and RalphX is polling GitHub
    pub fn waiting_on_pr(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::PlanBranchUpdateRequired => Response::Transition(State::UpdatingPlanBranch),
            TaskEvent::MergeComplete => Response::Transition(State::Merged),
            TaskEvent::MergeAgentError => Response::Transition(State::MergeIncomplete),
            TaskEvent::Retry => Response::Transition(State::PendingMerge),
            TaskEvent::Pause => Response::Transition(State::Paused),
            TaskEvent::Stop => Response::Transition(State::Stopped),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// Merging state - merge agent is attempting to resolve conflicts
    pub fn merging(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::PlanBranchUpdateRequired => Response::Transition(State::UpdatingPlanBranch),
            TaskEvent::MergeComplete => Response::Transition(State::Merged),
            TaskEvent::MergeAgentFailed => Response::Transition(State::MergeConflict),
            TaskEvent::MergeAgentError => Response::Transition(State::MergeIncomplete),
            TaskEvent::Retry => Response::Transition(State::PendingMerge),
            TaskEvent::Pause => Response::Transition(State::Paused),
            TaskEvent::Stop => Response::Transition(State::Stopped),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    pub fn branch_updating(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::Pause => Response::Transition(State::Paused),
            TaskEvent::Stop => Response::Transition(State::Stopped),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    pub fn branch_update_blocked(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::Pause => Response::Transition(State::Paused),
            TaskEvent::Stop => Response::Transition(State::Stopped),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// MergeIncomplete state - merge failed due to non-conflict errors
    /// Can retry (→ PendingMerge to re-attempt programmatic merge) or manually resolve (→ Merged)
    pub fn merge_incomplete(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::MergeConflict => Response::Transition(State::Merging),
            TaskEvent::ConflictResolved => Response::Transition(State::Merged),
            TaskEvent::Retry => Response::Transition(State::PendingMerge),
            TaskEvent::Pause => Response::Transition(State::Paused),
            TaskEvent::Stop => Response::Transition(State::Stopped),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// MergeConflict state - merge failed, needs manual resolution
    pub fn merge_conflict(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::Retry => Response::Transition(State::PendingMerge),
            TaskEvent::ConflictResolved => Response::Transition(State::Merged),
            TaskEvent::Pause => Response::Transition(State::Paused),
            TaskEvent::Stop => Response::Transition(State::Stopped),
            TaskEvent::Cancel => Response::Transition(State::Cancelled),
            _ => Response::NotHandled,
        }
    }

    /// Merged state - successfully merged to base branch
    pub fn merged(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::Retry => Response::Transition(State::Ready),
            _ => Response::NotHandled,
        }
    }

    // ==================
    // Terminal States
    // ==================

    /// Failed state - requires manual intervention
    pub fn failed(&mut self, event: &TaskEvent, _data: &FailedData) -> Response {
        match event {
            TaskEvent::Retry => {
                self.context.clear_error();
                Response::Transition(State::Ready)
            }
            _ => Response::NotHandled,
        }
    }

    /// Cancelled state - intentionally abandoned
    pub fn cancelled(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::Retry => Response::Transition(State::Ready),
            _ => Response::NotHandled,
        }
    }

    /// Stopped state - terminal, requires manual restart
    /// User explicitly stopped execution; task won't auto-resume
    pub fn stopped(&mut self, event: &TaskEvent) -> Response {
        match event {
            TaskEvent::Retry => Response::Transition(State::Ready),
            _ => Response::NotHandled,
        }
    }

    /// Paused state - non-terminal, can resume to previous state
    /// Resume uses status history to restore to the pre-pause agent-active state
    pub fn paused(&mut self, _event: &TaskEvent) -> Response {
        // Paused tasks are resumed via resume_execution command which uses
        // status history to restore to the pre-pause state.
        // The state machine doesn't handle resume directly - it's done at
        // the command layer via direct status transition.
        Response::NotHandled
    }
}
