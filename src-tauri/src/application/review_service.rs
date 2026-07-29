// ReviewService
// Application service for orchestrating review workflow: AI review, fix tasks, and escalation

use crate::domain::entities::{
    InternalStatus, ProjectId, Review, ReviewAction, ReviewActionType, ReviewNote, ReviewOutcome,
    ReviewerType, Task, TaskCategory, TaskId,
};
use crate::domain::repositories::{ReviewRepository, TaskRepository};
use crate::domain::review::config::ReviewSettings;
use crate::domain::tools::complete_review::{CompleteReviewInput, ReviewToolOutcome};
use crate::error::{AppError, AppResult};
use std::sync::Arc;

/// Service for orchestrating the review workflow
pub struct ReviewService<R: ReviewRepository, T: TaskRepository> {
    review_repo: Arc<R>,
    task_repo: Arc<T>,
    settings: ReviewSettings,
}

impl<R: ReviewRepository, T: TaskRepository> ReviewService<R, T> {
    /// Create a new review service with default settings
    pub fn new(review_repo: Arc<R>, task_repo: Arc<T>) -> Self {
        Self {
            review_repo,
            task_repo,
            settings: ReviewSettings::default(),
        }
    }

    /// Create a review service with custom settings
    pub fn with_settings(review_repo: Arc<R>, task_repo: Arc<T>, settings: ReviewSettings) -> Self {
        Self {
            review_repo,
            task_repo,
            settings,
        }
    }

    /// Start an AI review for a task
    ///
    /// Creates a Review record in Pending status for the given task.
    /// The actual reviewer agent should be spawned separately.
    pub async fn start_ai_review(
        &self,
        task_id: &TaskId,
        project_id: &ProjectId,
    ) -> AppResult<Review> {
        if !self.settings.should_run_ai_review() {
            return Err(AppError::Validation("AI review is disabled".into()));
        }

        // Check if task already has a pending review
        if self.review_repo.has_pending_review(task_id).await? {
            return Err(AppError::Validation(format!(
                "Task {} already has a pending review",
                task_id.as_str()
            )));
        }

        let review = Review::new(project_id.clone(), task_id.clone(), ReviewerType::Ai);
        self.review_repo.create(&review).await?;
        Ok(review)
    }

    /// Process the result of an AI review
    ///
    /// Handles the three possible outcomes: approved, needs_changes, escalate
    pub async fn process_review_result(
        &self,
        review: &mut Review,
        input: &CompleteReviewInput,
    ) -> AppResult<Option<TaskId>> {
        input
            .validate()
            .map_err(|e| AppError::Validation(e.to_string()))?;

        match input.outcome {
            ReviewToolOutcome::Approved => {
                review.approve(Some(input.notes.clone()));
                self.review_repo.update(review).await?;
                self.add_review_note(
                    &review.task_id,
                    ReviewerType::Ai,
                    ReviewOutcome::Approved,
                    &input.notes,
                )
                .await?;
                self.add_action(&review.id, ReviewActionType::Approved, None)
                    .await?;
                Ok(None)
            }
            // Phase 3 will implement the full approved_no_changes path (skip merge pipeline)
            ReviewToolOutcome::ApprovedNoChanges => {
                review.approve(Some(input.notes.clone()));
                self.review_repo.update(review).await?;
                self.add_review_note(
                    &review.task_id,
                    ReviewerType::Ai,
                    ReviewOutcome::ApprovedNoChanges,
                    &input.notes,
                )
                .await?;
                self.add_action(&review.id, ReviewActionType::Approved, None)
                    .await?;
                Ok(None)
            }
            ReviewToolOutcome::NeedsChanges => {
                let fix_desc = input
                    .fix_description
                    .as_ref()
                    .ok_or_else(|| AppError::Validation("Missing fix_description".into()))?;

                review.request_changes(input.notes.clone());
                self.review_repo.update(review).await?;
                self.add_review_note(
                    &review.task_id,
                    ReviewerType::Ai,
                    ReviewOutcome::ChangesRequested,
                    &input.notes,
                )
                .await?;

                if self.settings.should_auto_create_fix() {
                    let fix_task = self
                        .create_fix_task(&review.task_id, &review.project_id, fix_desc)
                        .await?;
                    self.add_action(
                        &review.id,
                        ReviewActionType::CreatedFixTask,
                        Some(fix_task.id.clone()),
                    )
                    .await?;
                    Ok(Some(fix_task.id))
                } else {
                    self.add_action(&review.id, ReviewActionType::MovedToBacklog, None)
                        .await?;
                    Ok(None)
                }
            }
            ReviewToolOutcome::Escalate => {
                review.reject(input.notes.clone());
                self.review_repo.update(review).await?;
                // Prefer escalation_reason (concise human explanation) over generic notes.
                let escalation_note = input
                    .escalation_reason
                    .as_deref()
                    .unwrap_or(&input.notes);
                // Legitimate AI decision — agent called complete_review(escalate). Do NOT change to System.
                self.add_review_note(
                    &review.task_id,
                    ReviewerType::Ai,
                    ReviewOutcome::Rejected,
                    escalation_note,
                )
                .await?;
                Ok(None)
            }
        }
    }

    /// Create a fix task for the original task
    pub async fn create_fix_task(
        &self,
        original_task_id: &TaskId,
        project_id: &ProjectId,
        fix_description: &str,
    ) -> AppResult<Task> {
        let original = self
            .task_repo
            .get_by_id(original_task_id)
            .await?
            .ok_or_else(|| AppError::TaskNotFound(original_task_id.as_str().to_string()))?;

        let mut fix_task = Task::new_with_category(
            project_id.clone(),
            format!("Fix: {}", original.title),
            TaskCategory::Regular,
        );
        fix_task.set_description(Some(fix_description.to_string()));
        fix_task.set_priority(original.priority + 1); // Higher priority than original

        if self.settings.needs_fix_approval() {
            fix_task.internal_status = InternalStatus::Blocked; // Pending human approval
        } else {
            fix_task.internal_status = InternalStatus::Ready; // Ready to execute
        }

        self.task_repo.create(fix_task).await
    }

    /// Add a review note to the task's history
    async fn add_review_note(
        &self,
        task_id: &TaskId,
        reviewer: ReviewerType,
        outcome: ReviewOutcome,
        notes: &str,
    ) -> AppResult<()> {
        let note = ReviewNote::with_notes(task_id.clone(), reviewer, outcome, notes.to_string());
        self.review_repo.add_note(&note).await
    }

    /// Add an action record to the review
    async fn add_action(
        &self,
        review_id: &crate::domain::entities::ReviewId,
        action_type: ReviewActionType,
        target_task_id: Option<TaskId>,
    ) -> AppResult<()> {
        let action = if let Some(target) = target_task_id {
            ReviewAction::with_target_task(review_id.clone(), action_type, target)
        } else {
            ReviewAction::new(review_id.clone(), action_type)
        };
        self.review_repo.add_action(&action).await
    }

    /// Get the current settings
    pub fn settings(&self) -> &ReviewSettings {
        &self.settings
    }

    async fn transition_task_validated(
        &self,
        task_id: &TaskId,
        target_status: InternalStatus,
        history_actor: &str,
    ) -> AppResult<Task> {
        self.transition_task_status(task_id, target_status, history_actor, true)
            .await
    }

    async fn transition_task_corrective(
        &self,
        task_id: &TaskId,
        target_status: InternalStatus,
        history_actor: &str,
    ) -> AppResult<Task> {
        self.transition_task_status(task_id, target_status, history_actor, false)
            .await
    }

    async fn transition_task_status(
        &self,
        task_id: &TaskId,
        target_status: InternalStatus,
        history_actor: &str,
        validate_graph: bool,
    ) -> AppResult<Task> {
        let mut task = self
            .task_repo
            .get_by_id(task_id)
            .await?
            .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;

        let from_status = task.internal_status;
        if from_status == target_status {
            return Ok(task);
        }

        if validate_graph && !from_status.can_transition_to(target_status) {
            return Err(AppError::InvalidTransition {
                from: from_status.as_str().to_string(),
                to: target_status.as_str().to_string(),
            });
        }

        task.internal_status = target_status;
        task.touch();

        if !self
            .task_repo
            .update_with_expected_status(&task, from_status)
            .await?
        {
            let current = self
                .task_repo
                .get_by_id(task_id)
                .await?
                .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;
            return Err(AppError::Conflict(format!(
                "Could not persist {} -> {} for task {}; current status is {}",
                from_status.as_str(),
                target_status.as_str(),
                task_id.as_str(),
                current.internal_status.as_str()
            )));
        }

        self.task_repo
            .persist_status_change(task_id, from_status, target_status, history_actor)
            .await?;

        Ok(task)
    }

    // ========================================
    // Fix Task Workflow Methods
    // ========================================

    /// Approve a fix task, changing its status from Blocked to Ready
    ///
    /// This is called when a human approves an AI-proposed fix task.
    pub async fn approve_fix_task(&self, fix_task_id: &TaskId) -> AppResult<()> {
        let fix_task = self
            .task_repo
            .get_by_id(fix_task_id)
            .await?
            .ok_or_else(|| AppError::TaskNotFound(fix_task_id.as_str().to_string()))?;

        if fix_task.internal_status != InternalStatus::Blocked {
            return Err(AppError::Validation(format!(
                "Fix task {} is not in Blocked status (current: {:?})",
                fix_task_id.as_str(),
                fix_task.internal_status
            )));
        }

        self.transition_task_validated(fix_task_id, InternalStatus::Ready, "review_fix")
            .await?;
        Ok(())
    }

    /// Reject a fix task with feedback and optionally create a new fix proposal
    ///
    /// If the original task has not exceeded max_fix_attempts, creates a new fix task
    /// with the provided feedback. Otherwise, moves the original task to backlog.
    ///
    /// Returns: Some(new_fix_task_id) if new fix created, None if max attempts reached
    pub async fn reject_fix_task(
        &self,
        fix_task_id: &TaskId,
        feedback: &str,
        original_task_id: &TaskId,
    ) -> AppResult<Option<TaskId>> {
        // Get fix task and mark as rejected
        let fix_task = self
            .task_repo
            .get_by_id(fix_task_id)
            .await?
            .ok_or_else(|| AppError::TaskNotFound(fix_task_id.as_str().to_string()))?;

        self.transition_task_corrective(fix_task_id, InternalStatus::Failed, "review_fix")
            .await?;

        // Get original task
        let original_task = self
            .task_repo
            .get_by_id(original_task_id)
            .await?
            .ok_or_else(|| AppError::TaskNotFound(original_task_id.as_str().to_string()))?;

        // Count fix attempts for original task
        let attempt_count = self.get_fix_attempt_count(original_task_id).await?;

        // Check if we've exceeded max attempts
        if self.settings.exceeded_max_attempts(attempt_count) {
            // Move original task to backlog
            self.transition_task_corrective(original_task_id, InternalStatus::Backlog, "review_fix")
                .await?;

            // Add a note about max attempts reached
            self.add_review_note(
                original_task_id,
                ReviewerType::System,
                ReviewOutcome::Rejected,
                &format!(
                    "Max fix attempts ({}) reached. Task moved to backlog. Last feedback: {}",
                    self.settings.max_fix_attempts, feedback
                ),
            )
            .await?;

            return Ok(None);
        }

        // Create new fix task with feedback
        let new_fix_description = format!(
            "Previous fix rejected. Feedback: {}\n\nOriginal issue: {}",
            feedback,
            fix_task.description.as_deref().unwrap_or("No description")
        );

        let new_fix_task = self
            .create_fix_task(
                original_task_id,
                &original_task.project_id,
                &new_fix_description,
            )
            .await?;

        Ok(Some(new_fix_task.id))
    }

    /// Get the number of fix task attempts for a task
    pub async fn get_fix_attempt_count(&self, task_id: &TaskId) -> AppResult<u32> {
        self.review_repo.count_fix_actions(task_id).await
    }

    /// Move a task to backlog (used when giving up on fixes)
    pub async fn move_to_backlog(&self, task_id: &TaskId, reason: &str) -> AppResult<()> {
        self.transition_task_corrective(task_id, InternalStatus::Backlog, "review_service")
            .await?;

        // Add a note about why it was moved to backlog
        self.add_review_note(task_id, ReviewerType::System, ReviewOutcome::Rejected, reason)
            .await
    }

    // ========================================
    // Human Review Methods
    // ========================================

    /// Start a human review for a task
    ///
    /// Creates a Review record in Pending status for manual human review.
    /// The task should already be in a state that requires human review
    /// (e.g., escalated from AI, or require_human_review is enabled).
    pub async fn start_human_review(
        &self,
        task_id: &TaskId,
        project_id: &ProjectId,
    ) -> AppResult<Review> {
        // Check if task already has a pending review
        if self.review_repo.has_pending_review(task_id).await? {
            return Err(AppError::Validation(format!(
                "Task {} already has a pending review",
                task_id.as_str()
            )));
        }

        // Verify task exists
        let _task = self
            .task_repo
            .get_by_id(task_id)
            .await?
            .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;

        let review = Review::new(project_id.clone(), task_id.clone(), ReviewerType::Human);
        self.review_repo.create(&review).await?;
        Ok(review)
    }

    /// Approve a human review
    ///
    /// Marks the review as approved with optional notes.
    /// The task should transition to the approved state.
    pub async fn approve_human_review(
        &self,
        review_id: &crate::domain::entities::ReviewId,
        notes: Option<String>,
    ) -> AppResult<()> {
        let mut review = self
            .review_repo
            .get_by_id(review_id)
            .await?
            .ok_or_else(|| {
                AppError::Validation(format!("Review {} not found", review_id.as_str()))
            })?;

        if !review.is_pending() {
            return Err(AppError::Validation(format!(
                "Review {} is not pending (current: {})",
                review_id.as_str(),
                review.status
            )));
        }

        review.approve(notes.clone());
        self.review_repo.update(&review).await?;

        // Add review note for history
        self.add_review_note(
            &review.task_id,
            ReviewerType::Human,
            ReviewOutcome::Approved,
            notes.as_deref().unwrap_or("Approved by human reviewer"),
        )
        .await?;

        // Add action record
        self.add_action(&review.id, ReviewActionType::Approved, None)
            .await
    }

    /// Request changes during human review
    ///
    /// Marks the review as changes_requested and optionally creates a fix task.
    /// Returns the fix task ID if one was created.
    pub async fn request_changes(
        &self,
        review_id: &crate::domain::entities::ReviewId,
        notes: String,
        fix_description: Option<String>,
    ) -> AppResult<Option<TaskId>> {
        let mut review = self
            .review_repo
            .get_by_id(review_id)
            .await?
            .ok_or_else(|| {
                AppError::Validation(format!("Review {} not found", review_id.as_str()))
            })?;

        if !review.is_pending() {
            return Err(AppError::Validation(format!(
                "Review {} is not pending (current: {})",
                review_id.as_str(),
                review.status
            )));
        }

        review.request_changes(notes.clone());
        self.review_repo.update(&review).await?;

        // Add review note for history
        self.add_review_note(
            &review.task_id,
            ReviewerType::Human,
            ReviewOutcome::ChangesRequested,
            &notes,
        )
        .await?;

        // Create fix task if description provided
        if let Some(fix_desc) = fix_description {
            let fix_task = self
                .create_fix_task(&review.task_id, &review.project_id, &fix_desc)
                .await?;
            self.add_action(
                &review.id,
                ReviewActionType::CreatedFixTask,
                Some(fix_task.id.clone()),
            )
            .await?;
            Ok(Some(fix_task.id))
        } else {
            Ok(None)
        }
    }

    /// Reject a human review
    ///
    /// Marks the review as rejected with notes.
    /// The task should transition to a failed/rejected state.
    pub async fn reject_human_review(
        &self,
        review_id: &crate::domain::entities::ReviewId,
        notes: String,
    ) -> AppResult<()> {
        let mut review = self
            .review_repo
            .get_by_id(review_id)
            .await?
            .ok_or_else(|| {
                AppError::Validation(format!("Review {} not found", review_id.as_str()))
            })?;

        if !review.is_pending() {
            return Err(AppError::Validation(format!(
                "Review {} is not pending (current: {})",
                review_id.as_str(),
                review.status
            )));
        }

        review.reject(notes.clone());
        self.review_repo.update(&review).await?;

        // Add review note for history
        self.add_review_note(
            &review.task_id,
            ReviewerType::Human,
            ReviewOutcome::Rejected,
            &notes,
        )
        .await?;

        // Move task to failed status
        self.transition_task_corrective(&review.task_id, InternalStatus::Failed, "review_human")
            .await?;
        Ok(())
    }
}
