// Tauri commands for Review operations
// Thin layer that delegates to ReviewRepository and ReviewService

use tauri::{Emitter, State};

use crate::application::AppState;
use crate::domain::entities::{
    ProjectId, ReviewId, ReviewNote, ReviewOutcome, ReviewerType, TaskCategory, TaskId,
};

// Re-export types for external use
pub use super::review_commands_types::{
    ApproveFixTaskInput, ApproveReviewInput, FixTaskAttemptsResponse, IssueProgressResponse,
    MarkIssueAddressedInput, MarkIssueInProgressInput, RejectFixTaskInput, RejectReviewInput,
    ReopenIssueInput, RequestChangesInput, ReviewActionResponse, ReviewIssueResponse,
    ReviewNoteResponse, ReviewResponse, ReviewSettingsResponse, UpdateReviewSettingsInput,
    VerifyIssueInput,
};

// ============================================================================
// Commands - Read operations
// ============================================================================

/// Get all pending reviews for a project
#[tauri::command]
pub async fn get_pending_reviews(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ReviewResponse>, String> {
    let project_id = ProjectId::from_string(project_id);
    state
        .review_repo
        .get_pending(&project_id)
        .await
        .map(|reviews| reviews.into_iter().map(ReviewResponse::from).collect())
        .map_err(|e| e.to_string())
}

/// Get a single review by ID
#[tauri::command]
pub async fn get_review_by_id(
    review_id: String,
    state: State<'_, AppState>,
) -> Result<Option<ReviewResponse>, String> {
    let review_id = ReviewId::from_string(review_id);
    state
        .review_repo
        .get_by_id(&review_id)
        .await
        .map(|opt| opt.map(ReviewResponse::from))
        .map_err(|e| e.to_string())
}

/// Get all reviews for a task
#[tauri::command]
pub async fn get_reviews_by_task_id(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ReviewResponse>, String> {
    let task_id = TaskId::from_string(task_id);
    state
        .review_repo
        .get_by_task_id(&task_id)
        .await
        .map(|reviews| reviews.into_iter().map(ReviewResponse::from).collect())
        .map_err(|e| e.to_string())
}

/// Get the state history (review notes) for a task
/// Parses embedded issues JSON from notes if present
#[tauri::command]
pub async fn get_task_state_history(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ReviewNoteResponse>, String> {
    let task_id = TaskId::from_string(task_id);
    state
        .review_repo
        .get_notes_by_task_id(&task_id)
        .await
        .map(|notes| notes.into_iter().map(ReviewNoteResponse::from).collect())
        .map_err(|e| e.to_string())
}

// ============================================================================
// Commands - Write operations (human review actions)
// ============================================================================

/// Approve a review
#[tauri::command]
pub async fn approve_review(
    input: ApproveReviewInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let review_id = ReviewId::from_string(input.review_id);

    // Get the review
    let mut review = state
        .review_repo
        .get_by_id(&review_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Review not found: {}", review_id.as_str()))?;

    // Check if pending
    if !review.is_pending() {
        return Err(format!(
            "Review {} is not pending (current: {})",
            review_id.as_str(),
            review.status
        ));
    }

    // Approve the review
    review.approve(input.notes);
    state
        .review_repo
        .update(&review)
        .await
        .map_err(|e| e.to_string())
}

/// Request changes on a review
#[tauri::command]
pub async fn request_changes(
    input: RequestChangesInput,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let review_id = ReviewId::from_string(input.review_id);

    // Get the review
    let mut review = state
        .review_repo
        .get_by_id(&review_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Review not found: {}", review_id.as_str()))?;

    // Check if pending
    if !review.is_pending() {
        return Err(format!(
            "Review {} is not pending (current: {})",
            review_id.as_str(),
            review.status
        ));
    }

    // Request changes
    review.request_changes(input.notes);
    state
        .review_repo
        .update(&review)
        .await
        .map_err(|e| e.to_string())?;

    // Return fix_description if provided (caller can use it to create fix task)
    Ok(input.fix_description)
}

/// Reject a review
#[tauri::command]
pub async fn reject_review(
    input: RejectReviewInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let review_id = ReviewId::from_string(input.review_id);

    // Get the review
    let mut review = state
        .review_repo
        .get_by_id(&review_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Review not found: {}", review_id.as_str()))?;

    // Check if pending
    if !review.is_pending() {
        return Err(format!(
            "Review {} is not pending (current: {})",
            review_id.as_str(),
            review.status
        ));
    }

    // Reject the review
    review.reject(input.notes);
    state
        .review_repo
        .update(&review)
        .await
        .map_err(|e| e.to_string())
}

// ============================================================================
// Commands - Fix task operations
// ============================================================================

use crate::domain::entities::{InternalStatus, Task};
use crate::domain::review::config::ReviewSettings;

/// Approve a fix task, changing its status from Blocked to Ready
#[tauri::command]
pub async fn approve_fix_task(
    input: ApproveFixTaskInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let fix_task_id = TaskId::from_string(input.fix_task_id);

    // Get the fix task
    let fix_task = state
        .task_repo
        .get_by_id(&fix_task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Fix task not found: {}", fix_task_id.as_str()))?;

    // Verify it's in Blocked status
    if fix_task.internal_status != InternalStatus::Blocked {
        return Err(format!(
            "Fix task {} is not in Blocked status (current: {:?})",
            fix_task_id.as_str(),
            fix_task.internal_status
        ));
    }

    let scheduler_concrete = Arc::new(
        state.build_task_scheduler_for_runtime(Arc::clone(&execution_state), Some(app.clone())),
    );
    scheduler_concrete.set_self_ref(Arc::clone(&scheduler_concrete) as Arc<dyn TaskScheduler>);
    let task_scheduler: Arc<dyn TaskScheduler> = scheduler_concrete;

    let transition_service = state
        .build_transition_service_with_execution_state(Arc::clone(&execution_state))
        .with_task_scheduler(task_scheduler);

    transition_service
        .transition_task(&fix_task_id, InternalStatus::Ready)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Reject a fix task with feedback, optionally creating a new fix proposal
/// Returns the new fix task ID if one was created, None if max attempts reached
#[tauri::command]
pub async fn reject_fix_task(
    input: RejectFixTaskInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    use crate::commands::emit_queue_changed;

    let fix_task_id = TaskId::from_string(input.fix_task_id);
    let original_task_id = TaskId::from_string(input.original_task_id);
    let settings = ReviewSettings::default();

    // Get fix task before corrective transition so we can preserve the old description
    let fix_task = state
        .task_repo
        .get_by_id(&fix_task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Fix task not found: {}", fix_task_id.as_str()))?;

    let transition_service =
        state.build_transition_service_with_execution_state(Arc::clone(&execution_state));

    transition_service
        .transition_task_corrective(&fix_task_id, InternalStatus::Failed, None, "review_fix")
        .await
        .map_err(|e| e.to_string())?;

    // Get original task
    let original_task = state
        .task_repo
        .get_by_id(&original_task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Original task not found: {}", original_task_id.as_str()))?;

    let project_id = original_task.project_id.clone();

    // Count fix attempts for original task
    let attempt_count = state
        .review_repo
        .count_fix_actions(&original_task_id)
        .await
        .map_err(|e| e.to_string())?;

    // Check if max attempts exceeded
    if settings.exceeded_max_attempts(attempt_count) {
        transition_service
            .transition_task_corrective(
                &original_task_id,
                InternalStatus::Backlog,
                None,
                "review_fix",
            )
            .await
            .map_err(|e| e.to_string())?;

        // Add review note about max attempts
        let note = ReviewNote::with_notes(
            original_task_id.clone(),
            ReviewerType::System,
            ReviewOutcome::Rejected,
            format!(
                "Max fix attempts ({}) reached. Task moved to backlog. Last feedback: {}",
                settings.max_fix_attempts, input.feedback
            ),
        );
        state
            .review_repo
            .add_note(&note)
            .await
            .map_err(|e| e.to_string())?;

        return Ok(None);
    }

    // Create new fix task with feedback
    let new_fix_description = format!(
        "Previous fix rejected. Feedback: {}\n\nOriginal issue: {}",
        input.feedback,
        fix_task.description.as_deref().unwrap_or("No description")
    );

    let mut new_fix_task = Task::new_with_category(
        project_id.clone(),
        format!("Fix: {}", original_task.title),
        TaskCategory::Regular,
    );
    new_fix_task.set_description(Some(new_fix_description));
    new_fix_task.set_priority(original_task.priority + 1);

    let should_emit_queue_changed = if settings.needs_fix_approval() {
        new_fix_task.internal_status = InternalStatus::Blocked;
        false
    } else {
        new_fix_task.internal_status = InternalStatus::Ready;
        true
    };

    let created = state
        .task_repo
        .create(new_fix_task)
        .await
        .map_err(|e| e.to_string())?;

    // Emit queue_changed if the new fix task is in Ready status
    if should_emit_queue_changed {
        emit_queue_changed(&state, &project_id, &app).await;
    }

    Ok(Some(created.id.as_str().to_string()))
}

/// Get the number of fix attempts for a task
#[tauri::command]
pub async fn get_fix_task_attempts(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<FixTaskAttemptsResponse, String> {
    let task_id = TaskId::from_string(task_id.clone());

    let attempt_count = state
        .review_repo
        .count_fix_actions(&task_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(FixTaskAttemptsResponse {
        task_id: task_id.as_str().to_string(),
        attempt_count,
    })
}

// ============================================================================
// Commands - Task-based approval (for human review after AI review)
// ============================================================================

use super::review_commands_types::{
    ApproveTaskInput, ReReviewTaskInput, RequestTaskChangesFromReviewingInput,
    RequestTaskChangesInput,
};
use crate::commands::execution_commands::ExecutionState;
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::{
    is_merge_worktree_path, restore_task_worktree,
};
use std::path::Path;
use std::sync::Arc;

/// Approve a task after AI review has passed or escalated
/// This is the human approval action that transitions the task to Approved status
#[tauri::command]
pub async fn approve_task_for_review(
    input: ApproveTaskInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let task_id = TaskId::from_string(input.task_id);

    // 1. Get task and validate state is ReviewPassed or Escalated
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    ensure_human_review_followup_status(task.internal_status, "approve")?;

    // 2. Create a human approval review note
    let review_note = ReviewNote::with_notes(
        task_id.clone(),
        ReviewerType::Human,
        ReviewOutcome::Approved,
        input
            .notes
            .unwrap_or_else(|| "Approved by user".to_string()),
    );
    state
        .review_repo
        .add_note(&review_note)
        .await
        .map_err(|e| e.to_string())?;

    // 3. Create scheduler for post-merge scheduling (Approved → PendingMerge path)
    let scheduler_concrete = Arc::new(
        state.build_task_scheduler_for_runtime(Arc::clone(&execution_state), Some(app.clone())),
    );
    scheduler_concrete.set_self_ref(Arc::clone(&scheduler_concrete) as Arc<dyn TaskScheduler>);
    let task_scheduler: Arc<dyn TaskScheduler> = scheduler_concrete;

    // 4. Transition to Approved using TaskTransitionService
    let transition_service = state
        .build_transition_service_with_execution_state(Arc::clone(&execution_state))
        .with_task_scheduler(task_scheduler);

    let old_status = task.internal_status.as_str().to_string();
    transition_service
        .transition_task(&task_id, InternalStatus::Approved)
        .await
        .map_err(|e| e.to_string())?;

    // 5. Emit events
    let _ = app.emit(
        "review:human_approved",
        serde_json::json!({
            "task_id": task_id.as_str(),
        }),
    );
    let _ = app.emit(
        "task:status_changed",
        serde_json::json!({
            "task_id": task_id.as_str(),
            "old_status": old_status,
            "new_status": "approved",
        }),
    );

    Ok(())
}

/// Request changes on a task after AI review has passed or escalated
/// This transitions the task to RevisionNeeded status for re-execution
#[tauri::command]
pub async fn request_task_changes_for_review(
    input: RequestTaskChangesInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let task_id = TaskId::from_string(input.task_id);

    // 1. Get task and validate state is ReviewPassed or Escalated
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    ensure_human_review_followup_status(task.internal_status, "request changes")?;

    // 2. Create a human changes-requested review note
    let review_note = ReviewNote::with_notes(
        task_id.clone(),
        ReviewerType::Human,
        ReviewOutcome::ChangesRequested,
        input.feedback.clone(),
    );
    state
        .review_repo
        .add_note(&review_note)
        .await
        .map_err(|e| e.to_string())?;

    // 3. Transition to RevisionNeeded (will auto-trigger re-execution)
    let transition_service =
        state.build_transition_service_with_execution_state(Arc::clone(&execution_state));

    let old_status = task.internal_status.as_str().to_string();
    transition_service
        .transition_task(&task_id, InternalStatus::RevisionNeeded)
        .await
        .map_err(|e| e.to_string())?;

    // 4. Emit events
    let _ = app.emit(
        "review:human_changes_requested",
        serde_json::json!({
            "task_id": task_id.as_str(),
            "feedback": input.feedback,
        }),
    );
    let _ = app.emit(
        "task:status_changed",
        serde_json::json!({
            "task_id": task_id.as_str(),
            "old_status": old_status,
            "new_status": "revision_needed",
        }),
    );

    Ok(())
}

/// Re-queue an escalated task for AI re-review
/// Transitions Escalated → PendingReview, skipping re-execution.
/// Use when the review was interrupted or the AI couldn't make a decision.
#[tauri::command]
pub async fn re_review_task_from_escalated(
    input: ReReviewTaskInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let task_id = TaskId::from_string(input.task_id);

    // 1. Get task and validate state is Escalated
    let mut task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    ensure_re_review_from_escalated_status(task.internal_status)?;

    // 1b. Restore worktree_path if it's stale (pointing to a merge worktree).
    //     This unblocks tasks stuck after a merge-pipeline conflict routed them back
    //     to review without resetting the worktree path.
    if let Some(ref wt_path) = task.worktree_path.clone() {
        if is_merge_worktree_path(wt_path) {
            let project = state
                .project_repo
                .get_by_id(&task.project_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Project not found for task {}", task_id.as_str()))?;
            let repo_path = Path::new(&project.working_directory).to_path_buf();
            restore_task_worktree(&mut task, &project, &repo_path)
                .await
                .map_err(|e| e.to_string())?;
            state
                .task_repo
                .update(&task)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    // 2. Transition to PendingReview (state machine auto-triggers AI reviewer)
    let transition_service =
        state.build_transition_service_with_execution_state(Arc::clone(&execution_state));

    let old_status = task.internal_status.as_str().to_string();
    transition_service
        .transition_task(&task_id, InternalStatus::PendingReview)
        .await
        .map_err(|e| e.to_string())?;

    // 3. Emit events
    let _ = app.emit(
        "review:re_review_requested",
        serde_json::json!({
            "task_id": task_id.as_str(),
        }),
    );
    let _ = app.emit(
        "task:status_changed",
        serde_json::json!({
            "task_id": task_id.as_str(),
            "old_status": old_status,
            "new_status": "pending_review",
        }),
    );

    Ok(())
}

fn ensure_human_review_followup_status(
    status: InternalStatus,
    action: &str,
) -> Result<(), String> {
    if matches!(status, InternalStatus::ReviewPassed | InternalStatus::Escalated) {
        return Ok(());
    }

    Err(format!(
        "Task must be in 'review_passed' or 'escalated' status to {}. Current status: {}. \
        This action is only available after the AI reviewer has approved or escalated the task.",
        action,
        status.as_str()
    ))
}

fn ensure_re_review_from_escalated_status(status: InternalStatus) -> Result<(), String> {
    if status == InternalStatus::Escalated {
        return Ok(());
    }

    Err(format!(
        "Task must be in 'escalated' status to re-review. Current status: {}.",
        status.as_str()
    ))
}

/// Request changes on a task while it is actively being reviewed (Reviewing state)
///
/// Atomically: stops the reviewer agent → writes idempotency flag → adds a human
/// ChangesRequested review note → transitions to RevisionNeeded → emits events.
/// Emits `review:action_failed` on any failure in steps 3–5.
#[tauri::command]
pub async fn request_task_changes_from_reviewing(
    input: RequestTaskChangesFromReviewingInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use crate::application::ChatService as _;
    use crate::commands::unified_chat_commands::create_chat_service;
    use crate::domain::entities::ChatContextType;
    use crate::domain::state_machine::transition_handler::parse_metadata;

    let task_id = TaskId::from_string(input.task_id);

    // 1. Get task and validate state is Reviewing
    let mut task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    if task.internal_status != InternalStatus::Reviewing {
        return Err(format!(
            "Task must be in 'reviewing' status to request changes from an active review. \
            Current status: {}. Use request_task_changes_for_review for tasks in \
            'review_passed' or 'escalated' status.",
            task.internal_status.as_str()
        ));
    }

    // 2. Write idempotency guard to metadata before any side effects
    //    Prevents replay of RevisionNeeded → ReExecuting auto-transition on restart.
    {
        let mut meta = parse_metadata(&task).unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = meta.as_object_mut() {
            obj.insert(
                "request_changes_initiated".to_string(),
                serde_json::json!(true),
            );
        }
        task.metadata = Some(
            serde_json::to_string(&meta)
                .unwrap_or_else(|_| r#"{"request_changes_initiated":true}"#.to_string()),
        );
        task.touch();
        state
            .task_repo
            .update(&task)
            .await
            .map_err(|e| e.to_string())?;
    }

    let emit_action_failed = |reason: &str| {
        let _ = app.emit(
            "review:action_failed",
            serde_json::json!({
                "task_id": task_id.as_str(),
                "reason": reason,
            }),
        );
    };

    // 3. Stop the reviewer agent (SIGTERM + mark agent_run failed + agent:stopped event)
    let chat_service = create_chat_service(&state, app.clone(), &execution_state, None);
    if let Err(e) = chat_service
        .stop_agent(ChatContextType::Review, task_id.as_str())
        .await
    {
        let msg = e.to_string();
        emit_action_failed(&msg);
        return Err(msg);
    }

    // 4. Add a human ChangesRequested review note
    let review_note = ReviewNote::with_notes(
        task_id.clone(),
        ReviewerType::Human,
        ReviewOutcome::ChangesRequested,
        input.feedback.clone(),
    );
    if let Err(e) = state.review_repo.add_note(&review_note).await {
        let msg = e.to_string();
        emit_action_failed(&msg);
        return Err(msg);
    }

    // 5. Transition to RevisionNeeded (auto-chain fires RevisionNeeded → ReExecuting)
    let transition_service =
        state.build_transition_service_with_execution_state(Arc::clone(&execution_state));

    if let Err(e) = transition_service
        .transition_task(&task_id, InternalStatus::RevisionNeeded)
        .await
    {
        let msg = e.to_string();
        emit_action_failed(&msg);
        return Err(msg);
    }

    // 6. Emit success events
    let _ = app.emit(
        "review:human_changes_requested",
        serde_json::json!({
            "task_id": task_id.as_str(),
            "feedback": input.feedback,
        }),
    );
    let _ = app.emit(
        "task:status_changed",
        serde_json::json!({
            "task_id": task_id.as_str(),
            "old_status": "reviewing",
            "new_status": "revision_needed",
        }),
    );

    Ok(())
}

// ============================================================================
// Commands - Review Issue operations
// ============================================================================

use crate::domain::entities::ReviewIssueId;
use crate::domain::entities::ReviewNoteId;

/// Get issues for a task, optionally filtered by status
/// status_filter: "open" returns only open issues, "all" or None returns all issues
#[tauri::command]
pub async fn get_task_issues(
    task_id: String,
    status_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ReviewIssueResponse>, String> {
    let task_id = TaskId::from_string(task_id);

    let issues = match status_filter.as_deref() {
        Some("open") => state
            .review_issue_repo
            .get_open_by_task_id(&task_id)
            .await
            .map_err(|e| e.to_string())?,
        _ => state
            .review_issue_repo
            .get_by_task_id(&task_id)
            .await
            .map_err(|e| e.to_string())?,
    };

    Ok(issues.into_iter().map(ReviewIssueResponse::from).collect())
}

/// Get issue progress summary for a task
#[tauri::command]
pub async fn get_issue_progress(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<IssueProgressResponse, String> {
    let task_id = TaskId::from_string(task_id);

    let summary = state
        .review_issue_repo
        .get_summary(&task_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(IssueProgressResponse::from(summary))
}

#[cfg(test)]
mod transition_guard_tests {
    use super::*;

    #[test]
    fn human_review_followup_accepts_review_passed_and_escalated() {
        assert!(ensure_human_review_followup_status(InternalStatus::ReviewPassed, "approve").is_ok());
        assert!(
            ensure_human_review_followup_status(InternalStatus::Escalated, "request changes")
                .is_ok()
        );
    }

    #[test]
    fn human_review_followup_rejects_terminal_statuses() {
        let error = ensure_human_review_followup_status(InternalStatus::Merged, "approve")
            .expect_err("merged task must be rejected");
        assert!(error.contains("review_passed"));
        assert!(error.contains("escalated"));
        assert!(error.contains("merged"));
    }

    #[test]
    fn rereview_requires_escalated_status() {
        assert!(ensure_re_review_from_escalated_status(InternalStatus::Escalated).is_ok());

        let error = ensure_re_review_from_escalated_status(InternalStatus::ReviewPassed)
            .expect_err("review_passed task must be rejected");
        assert!(error.contains("escalated"));
        assert!(error.contains("review_passed"));
    }
}

/// Verify that an issue has been fixed (Addressed -> Verified)
/// Called by the reviewer during subsequent review
#[tauri::command]
pub async fn verify_issue(
    input: VerifyIssueInput,
    state: State<'_, AppState>,
) -> Result<ReviewIssueResponse, String> {
    let issue_id = ReviewIssueId::from_string(input.issue_id);
    let review_note_id = ReviewNoteId::from_string(input.review_note_id);

    let mut issue = state
        .review_issue_repo
        .get_by_id(&issue_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Issue not found: {}", issue_id.as_str()))?;

    if issue.status != crate::domain::entities::IssueStatus::Addressed {
        return Err(format!(
            "Cannot verify issue: current status is {} (expected addressed)",
            issue.status
        ));
    }

    issue.verify(review_note_id);
    state
        .review_issue_repo
        .update(&issue)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ReviewIssueResponse::from(issue))
}

/// Reopen an issue that was not actually fixed (Addressed -> Open)
/// Called by the reviewer during subsequent review when fix is insufficient
#[tauri::command]
pub async fn reopen_issue(
    input: ReopenIssueInput,
    state: State<'_, AppState>,
) -> Result<ReviewIssueResponse, String> {
    let issue_id = ReviewIssueId::from_string(input.issue_id);

    let mut issue = state
        .review_issue_repo
        .get_by_id(&issue_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Issue not found: {}", issue_id.as_str()))?;

    if issue.status != crate::domain::entities::IssueStatus::Addressed {
        return Err(format!(
            "Cannot reopen issue: current status is {} (expected addressed)",
            issue.status
        ));
    }

    issue.reopen(input.reason);
    state
        .review_issue_repo
        .update(&issue)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ReviewIssueResponse::from(issue))
}

// ============================================================================
// Commands - Execute Agent Issue Tracking
// ============================================================================

/// Mark an issue as being worked on (Open -> InProgress)
/// Called by the worker/execute agent when starting to work on an issue
#[tauri::command]
pub async fn mark_issue_in_progress(
    input: MarkIssueInProgressInput,
    state: State<'_, AppState>,
) -> Result<ReviewIssueResponse, String> {
    let issue_id = ReviewIssueId::from_string(input.issue_id);

    let mut issue = state
        .review_issue_repo
        .get_by_id(&issue_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Issue not found: {}", issue_id.as_str()))?;

    if issue.status != crate::domain::entities::IssueStatus::Open {
        return Err(format!(
            "Cannot mark issue as in_progress: current status is {} (expected open)",
            issue.status
        ));
    }

    issue.start_work();
    state
        .review_issue_repo
        .update(&issue)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ReviewIssueResponse::from(issue))
}

/// Mark an issue as addressed (Open/InProgress -> Addressed)
/// Called by the worker/execute agent after completing work on an issue
#[tauri::command]
pub async fn mark_issue_addressed(
    input: MarkIssueAddressedInput,
    state: State<'_, AppState>,
) -> Result<ReviewIssueResponse, String> {
    let issue_id = ReviewIssueId::from_string(input.issue_id);

    let mut issue = state
        .review_issue_repo
        .get_by_id(&issue_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Issue not found: {}", issue_id.as_str()))?;

    if !issue.needs_work() {
        return Err(format!(
            "Cannot mark issue as addressed: current status is {} (expected open or in_progress)",
            issue.status
        ));
    }

    issue.mark_addressed(Some(input.resolution_notes), input.attempt_number);
    state
        .review_issue_repo
        .update(&issue)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ReviewIssueResponse::from(issue))
}

// ============================================================================
// Commands - Review Settings (global policy)
// ============================================================================

/// Get the global review policy settings
#[tauri::command]
pub async fn get_review_settings(
    state: State<'_, AppState>,
) -> Result<ReviewSettingsResponse, String> {
    let settings = state
        .review_settings_repo
        .get_settings()
        .await
        .map_err(|e| e.to_string())?;

    Ok(ReviewSettingsResponse::from(settings))
}

/// Update the global review policy settings.
/// Only the fields provided in the input are changed; ballast fields are preserved.
#[tauri::command]
pub async fn update_review_settings(
    input: UpdateReviewSettingsInput,
    state: State<'_, AppState>,
) -> Result<ReviewSettingsResponse, String> {
    let mut settings = state
        .review_settings_repo
        .get_settings()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(v) = input.require_human_review {
        settings.require_human_review = v;
    }
    if let Some(v) = input.max_fix_attempts {
        settings.max_fix_attempts = v;
    }
    if let Some(v) = input.max_revision_cycles {
        settings.max_revision_cycles = v;
    }

    let updated = state
        .review_settings_repo
        .update_settings(&settings)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ReviewSettingsResponse::from(updated))
}
