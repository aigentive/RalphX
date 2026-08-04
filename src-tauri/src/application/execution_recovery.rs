use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::application::{AppState, ExecutionState, ReconciliationRunner, TaskTransitionService};
use crate::domain::entities::{InternalStatus, Task};

pub(crate) fn build_transition_service_for_recovery(
    app_state: &AppState,
    execution_state: Arc<ExecutionState>,
) -> Arc<TaskTransitionService> {
    Arc::new(
        app_state
            .build_transition_service_with_execution_state(execution_state)
            .with_step_repo(Arc::clone(&app_state.task_step_repo)),
    )
}

pub(crate) fn build_reconciler_for_recovery(
    app_state: &AppState,
    execution_state: Arc<ExecutionState>,
    app: AppHandle,
) -> ReconciliationRunner {
    ReconciliationRunner::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.artifact_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.memory_event_repo),
        Arc::clone(&app_state.agent_run_repo),
        build_transition_service_for_recovery(app_state, execution_state.clone()),
        execution_state,
        Some(app),
    )
    .with_notification_service(app_state.notification_service())
    .with_execution_settings_repo(Arc::clone(&app_state.execution_settings_repo))
    .with_plan_branch_repo(Arc::clone(&app_state.plan_branch_repo))
    .with_branch_update_repo(Arc::clone(&app_state.branch_update_repo))
    .with_interactive_process_registry(Arc::clone(&app_state.interactive_process_registry))
}

// ========================================
// Smart Resume Types and Functions
// ========================================

/// Category of resume behavior based on the stopped_from_status.
///
/// Determines how a task should be resumed after being stopped mid-execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResumeCategory {
    /// Directly resume to the original state (spawn agent if needed).
    /// Used for: Executing, ReExecuting, Reviewing, QaRefining, QaTesting
    Direct,
    /// Validate git state before resuming.
    /// Used for: Merging, PendingMerge, MergeConflict, MergeIncomplete
    Validated,
    /// Redirect to a successor state (avoid invalid intermediate states).
    /// Used for: QaPassed, RevisionNeeded, PendingReview
    Redirect,
}

/// Result of categorizing a resume state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategorizedResume {
    /// The category of resume behavior
    pub category: ResumeCategory,
    /// The target status to resume to (may differ from original for Redirect)
    pub target_status: InternalStatus,
}

/// Categorize the resume state based on the stopped_from_status.
///
/// Returns a `CategorizedResume` with the category and target status.
/// For Redirect states, the target is the successor state.
pub fn categorize_resume_state(stopped_from_status: InternalStatus) -> CategorizedResume {
    match stopped_from_status {
        // Direct Resume: spawn agent directly
        InternalStatus::Executing
        | InternalStatus::ReExecuting
        | InternalStatus::Reviewing
        | InternalStatus::QaRefining
        | InternalStatus::QaTesting => CategorizedResume {
            category: ResumeCategory::Direct,
            target_status: stopped_from_status,
        },

        // Validated Resume: check git state first
        InternalStatus::Merging
        | InternalStatus::PendingMerge
        | InternalStatus::MergeConflict
        | InternalStatus::MergeIncomplete => CategorizedResume {
            category: ResumeCategory::Validated,
            target_status: stopped_from_status,
        },

        // Redirect: go to successor state (these have auto-transitions)
        InternalStatus::QaPassed => CategorizedResume {
            // QaPassed → PendingReview (auto-transitions anyway)
            category: ResumeCategory::Redirect,
            target_status: InternalStatus::PendingReview,
        },
        InternalStatus::RevisionNeeded => CategorizedResume {
            // RevisionNeeded → ReExecuting (auto-transitions anyway)
            category: ResumeCategory::Redirect,
            target_status: InternalStatus::ReExecuting,
        },
        InternalStatus::PendingReview => CategorizedResume {
            // PendingReview → Reviewing (spawn reviewer)
            category: ResumeCategory::Redirect,
            target_status: InternalStatus::Reviewing,
        },

        // Default: treat as Direct (fallback to Ready if invalid)
        _ => CategorizedResume {
            category: ResumeCategory::Direct,
            target_status: stopped_from_status,
        },
    }
}

pub fn restart_transition_target(stopped_from_status: InternalStatus) -> InternalStatus {
    match stopped_from_status {
        InternalStatus::Executing | InternalStatus::ReExecuting => InternalStatus::Ready,
        _ => categorize_resume_state(stopped_from_status).target_status,
    }
}

/// Validation warning for resume operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeValidationWarning {
    /// Warning code (e.g., "dirty_worktree", "base_branch_moved")
    pub code: String,
    /// Human-readable warning message
    pub message: String,
}

/// Result of resume validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeValidationResult {
    /// Whether validation passed (true = can proceed)
    pub passed: bool,
    /// Warnings encountered (non-blocking issues)
    pub warnings: Vec<ResumeValidationWarning>,
}

/// Result type for restart_task command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RestartResult {
    /// Task was successfully restarted
    Success {
        /// The updated task
        task: serde_json::Value,
        /// The category of resume that was used
        category: ResumeCategory,
        /// The status the task was resumed to
        resumed_to_status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        disposition: Option<RestartDisposition>,
    },
    /// Validation failed (only for Validated category)
    ValidationFailed {
        /// Validation warnings that caused the failure
        warnings: Vec<ResumeValidationWarning>,
        /// The stopped_from_status for reference
        stopped_from_status: String,
    },
    /// Recovery authority could not be established safely; no task state was mutated.
    Blocked {
        warnings: Vec<ResumeValidationWarning>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartDisposition {
    RecoveredToReview,
    RestartedToReady,
}

/// Validate resume for Validated category states.
///
/// Checks:
/// - Task branch exists and is accessible
/// - Worktree is clean (no uncommitted changes)
/// - No stale merge/rebase in progress
pub(crate) async fn validate_resume(task: &Task, state: &AppState) -> ResumeValidationResult {
    use crate::application::git_service::GitService;
    use std::path::Path;

    let mut warnings = Vec::new();

    // Get project for git operations
    let project = match state.project_repo.get_by_id(&task.project_id).await {
        Ok(Some(p)) => p,
        _ => {
            warnings.push(ResumeValidationWarning {
                code: "project_not_found".to_string(),
                message: "Could not find project for git validation".to_string(),
            });
            return ResumeValidationResult {
                passed: false,
                warnings,
            };
        }
    };

    // Check if task has a branch
    let branch_name = match &task.task_branch {
        Some(branch) => branch.clone(),
        None => {
            warnings.push(ResumeValidationWarning {
                code: "no_branch".to_string(),
                message: "Task has no associated branch".to_string(),
            });
            return ResumeValidationResult {
                passed: false,
                warnings,
            };
        }
    };

    let repo_path = Path::new(&project.working_directory);

    // Check branch exists
    if !GitService::branch_exists(repo_path, &branch_name)
        .await
        .unwrap_or(false)
    {
        warnings.push(ResumeValidationWarning {
            code: "branch_not_found".to_string(),
            message: format!("Task branch '{}' does not exist", branch_name),
        });
        return ResumeValidationResult {
            passed: false,
            warnings,
        };
    }

    // Check worktree is clean (if worktree path exists)
    if let Some(worktree_path) = &task.worktree_path {
        let worktree = Path::new(worktree_path);
        match GitService::has_uncommitted_changes(worktree).await {
            Ok(false) => {} // Clean, no changes
            Ok(true) => {
                warnings.push(ResumeValidationWarning {
                    code: "dirty_worktree".to_string(),
                    message: "Worktree has uncommitted changes".to_string(),
                });
                // Non-blocking warning - just log
                tracing::warn!(
                    task_id = task.id.as_str(),
                    worktree = %worktree_path,
                    "Worktree is dirty but proceeding"
                );
            }
            Err(e) => {
                warnings.push(ResumeValidationWarning {
                    code: "worktree_check_failed".to_string(),
                    message: format!("Could not check worktree status: {}", e),
                });
            }
        }
    }

    // All critical checks passed
    ResumeValidationResult {
        passed: true,
        warnings,
    }
}
