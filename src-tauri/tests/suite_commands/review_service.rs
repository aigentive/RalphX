use async_trait::async_trait;
use ralphx_lib::application::ReviewService;
use ralphx_lib::domain::entities::{self, *};
use ralphx_lib::domain::repositories::{self, *};
use ralphx_lib::domain::review::config::ReviewSettings;
use ralphx_lib::domain::tools::{CompleteReviewInput, ReviewIssueInput};
use ralphx_lib::error::{AppError, AppResult};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

// Mock ReviewRepository
struct MockReviewRepo {
    reviews: RwLock<HashMap<String, Review>>,
    actions: RwLock<Vec<ReviewAction>>,
    notes: RwLock<Vec<ReviewNote>>,
}

impl MockReviewRepo {
    fn new() -> Self {
        Self {
            reviews: RwLock::new(HashMap::new()),
            actions: RwLock::new(Vec::new()),
            notes: RwLock::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ReviewRepository for MockReviewRepo {
    async fn create(&self, review: &Review) -> AppResult<()> {
        self.reviews
            .write()
            .unwrap()
            .insert(review.id.as_str().to_string(), review.clone());
        Ok(())
    }
    async fn get_by_id(&self, id: &ReviewId) -> AppResult<Option<Review>> {
        Ok(self.reviews.read().unwrap().get(id.as_str()).cloned())
    }
    async fn get_by_task_id(&self, task_id: &TaskId) -> AppResult<Vec<Review>> {
        Ok(self
            .reviews
            .read()
            .unwrap()
            .values()
            .filter(|r| r.task_id == *task_id)
            .cloned()
            .collect())
    }
    async fn get_pending(&self, _project_id: &ProjectId) -> AppResult<Vec<Review>> {
        Ok(vec![])
    }
    async fn update(&self, review: &Review) -> AppResult<()> {
        self.reviews
            .write()
            .unwrap()
            .insert(review.id.as_str().to_string(), review.clone());
        Ok(())
    }
    async fn delete(&self, _id: &ReviewId) -> AppResult<()> {
        Ok(())
    }
    async fn add_action(&self, action: &ReviewAction) -> AppResult<()> {
        self.actions.write().unwrap().push(action.clone());
        Ok(())
    }
    async fn get_actions(&self, _review_id: &ReviewId) -> AppResult<Vec<ReviewAction>> {
        Ok(vec![])
    }
    async fn get_action_by_id(
        &self,
        _id: &entities::ReviewActionId,
    ) -> AppResult<Option<ReviewAction>> {
        Ok(None)
    }
    async fn add_note(&self, note: &ReviewNote) -> AppResult<()> {
        self.notes.write().unwrap().push(note.clone());
        Ok(())
    }
    async fn get_notes_by_task_id(&self, task_id: &TaskId) -> AppResult<Vec<ReviewNote>> {
        Ok(self
            .notes
            .read()
            .unwrap()
            .iter()
            .filter(|n| n.task_id == *task_id)
            .cloned()
            .collect())
    }
    async fn get_note_by_id(&self, _id: &entities::ReviewNoteId) -> AppResult<Option<ReviewNote>> {
        Ok(None)
    }
    async fn get_by_status(
        &self,
        _project_id: &ProjectId,
        _status: ReviewStatus,
    ) -> AppResult<Vec<Review>> {
        Ok(vec![])
    }
    async fn count_pending(&self, _project_id: &ProjectId) -> AppResult<u32> {
        Ok(0)
    }
    async fn has_pending_review(&self, task_id: &TaskId) -> AppResult<bool> {
        Ok(self
            .reviews
            .read()
            .unwrap()
            .values()
            .any(|r| r.task_id == *task_id && r.is_pending()))
    }
    async fn count_fix_actions(&self, task_id: &TaskId) -> AppResult<u32> {
        let reviews = self.reviews.read().unwrap();
        let actions = self.actions.read().unwrap();
        let mut count = 0u32;
        for review in reviews.values() {
            if review.task_id == *task_id {
                for action in actions.iter() {
                    if action.review_id == review.id
                        && action.action_type == entities::ReviewActionType::CreatedFixTask
                    {
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    }
    async fn get_fix_actions(&self, task_id: &TaskId) -> AppResult<Vec<ReviewAction>> {
        let reviews = self.reviews.read().unwrap();
        let actions = self.actions.read().unwrap();
        let mut result = Vec::new();
        for review in reviews.values() {
            if review.task_id == *task_id {
                for action in actions.iter() {
                    if action.review_id == review.id
                        && action.action_type == entities::ReviewActionType::CreatedFixTask
                    {
                        result.push(action.clone());
                    }
                }
            }
        }
        Ok(result)
    }
}

// Mock TaskRepository
struct MockTaskRepo {
    tasks: RwLock<HashMap<String, Task>>,
    status_history: RwLock<HashMap<String, Vec<repositories::StatusTransition>>>,
}

impl MockTaskRepo {
    fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            status_history: RwLock::new(HashMap::new()),
        }
    }
    fn add_task(&self, task: Task) {
        self.tasks
            .write()
            .unwrap()
            .insert(task.id.as_str().to_string(), task);
    }
}

#[async_trait]
impl TaskRepository for MockTaskRepo {
    async fn create(&self, task: Task) -> AppResult<Task> {
        self.tasks
            .write()
            .unwrap()
            .insert(task.id.as_str().to_string(), task.clone());
        Ok(task)
    }
    async fn get_by_id(&self, id: &TaskId) -> AppResult<Option<Task>> {
        Ok(self.tasks.read().unwrap().get(id.as_str()).cloned())
    }
    async fn get_by_project(&self, _project_id: &ProjectId) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn update(&self, task: &Task) -> AppResult<()> {
        self.tasks
            .write()
            .unwrap()
            .insert(task.id.as_str().to_string(), task.clone());
        Ok(())
    }
    async fn update_with_expected_status(
        &self,
        task: &Task,
        expected_status: InternalStatus,
    ) -> AppResult<bool> {
        let mut tasks = self.tasks.write().unwrap();
        if let Some(existing) = tasks.get(task.id.as_str()) {
            if existing.internal_status != expected_status {
                return Ok(false);
            }
        } else {
            return Ok(false);
        }
        tasks.insert(task.id.as_str().to_string(), task.clone());
        Ok(true)
    }
    async fn update_metadata(&self, _id: &TaskId, _metadata: Option<String>) -> AppResult<()> {
        Ok(())
    }
    async fn delete(&self, _id: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn get_by_status(
        &self,
        _project_id: &ProjectId,
        _status: InternalStatus,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn persist_status_change(
        &self,
        id: &TaskId,
        from: InternalStatus,
        to: InternalStatus,
        trigger: &str,
    ) -> AppResult<String> {
        self.status_history
            .write()
            .unwrap()
            .entry(id.as_str().to_string())
            .or_default()
            .push(repositories::StatusTransition::new(from, to, trigger));
        Ok(uuid::Uuid::new_v4().to_string())
    }
    async fn get_status_history(
        &self,
        id: &TaskId,
    ) -> AppResult<Vec<repositories::StatusTransition>> {
        Ok(self
            .status_history
            .read()
            .unwrap()
            .get(id.as_str())
            .cloned()
            .unwrap_or_default())
    }
    async fn get_status_entered_at(
        &self,
        _task_id: &TaskId,
        _status: InternalStatus,
    ) -> AppResult<Option<chrono::DateTime<chrono::Utc>>> {
        Ok(None)
    }

    async fn get_status_last_entered_at(
        &self,
        _task_id: &TaskId,
        _status: InternalStatus,
    ) -> AppResult<Option<chrono::DateTime<chrono::Utc>>> {
        Ok(None)
    }

    async fn get_next_executable(&self, _project_id: &ProjectId) -> AppResult<Option<Task>> {
        Ok(None)
    }
    async fn get_by_ideation_session(
        &self,
        _session_id: &entities::IdeationSessionId,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn get_by_project_filtered(
        &self,
        _project_id: &ProjectId,
        _include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn archive(&self, task_id: &TaskId) -> AppResult<Task> {
        if let Some(task) = self.tasks.read().unwrap().get(task_id.as_str()) {
            let mut archived = task.clone();
            archived.archived_at = Some(chrono::Utc::now());
            self.tasks
                .write()
                .unwrap()
                .insert(task_id.as_str().to_string(), archived.clone());
            Ok(archived)
        } else {
            Err(AppError::NotFound(format!(
                "Task {} not found",
                task_id.as_str()
            )))
        }
    }
    async fn restore(&self, task_id: &TaskId) -> AppResult<Task> {
        if let Some(task) = self.tasks.read().unwrap().get(task_id.as_str()) {
            let mut restored = task.clone();
            restored.archived_at = None;
            self.tasks
                .write()
                .unwrap()
                .insert(task_id.as_str().to_string(), restored.clone());
            Ok(restored)
        } else {
            Err(AppError::NotFound(format!(
                "Task {} not found",
                task_id.as_str()
            )))
        }
    }
    async fn get_archived_count(
        &self,
        _project_id: &ProjectId,
        _ideation_session_id: Option<&str>,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn list_paginated(
        &self,
        _project_id: &ProjectId,
        _statuses: Option<Vec<InternalStatus>>,
        _offset: u32,
        _limit: u32,
        _include_archived: bool,
        _ideation_session_id: Option<&str>,
        _execution_plan_id: Option<&str>,
        _categories: Option<&[String]>,
    ) -> AppResult<Vec<Task>> {
        Ok(self.tasks.read().unwrap().values().cloned().collect())
    }

    async fn count_tasks(
        &self,
        _project_id: &ProjectId,
        _include_archived: bool,
        _ideation_session_id: Option<&str>,
        _execution_plan_id: Option<&str>,
    ) -> AppResult<u32> {
        Ok(self.tasks.read().unwrap().len() as u32)
    }

    async fn search(
        &self,
        _project_id: &ProjectId,
        _query: &str,
        _include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn get_oldest_ready_task(&self) -> AppResult<Option<Task>> {
        Ok(None)
    }

    async fn get_oldest_ready_tasks(&self, _limit: u32) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn get_stale_ready_tasks(&self, _threshold_secs: u64) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn update_latest_state_history_metadata(
        &self,
        _task_id: &TaskId,
        _metadata: &StateHistoryMetadata,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn has_task_in_states(
        &self,
        _project_id: &ProjectId,
        _statuses: &[InternalStatus],
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn get_status_history_batch(
        &self,
        task_ids: &[entities::TaskId],
    ) -> AppResult<HashMap<entities::TaskId, Vec<repositories::StatusTransition>>> {
        let history = self.status_history.read().unwrap();
        Ok(task_ids
            .iter()
            .filter_map(|task_id| {
                history
                    .get(task_id.as_str())
                    .cloned()
                    .map(|entries| (task_id.clone(), entries))
            })
            .collect())
    }
}

fn setup() -> (Arc<MockReviewRepo>, Arc<MockTaskRepo>, ProjectId, TaskId) {
    let review_repo = Arc::new(MockReviewRepo::new());
    let task_repo = Arc::new(MockTaskRepo::new());
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());
    let task = Task::new(project_id.clone(), "Original Task".to_string());
    let mut task_with_id = task;
    task_with_id.id = task_id.clone();
    task_repo.add_task(task_with_id);
    (review_repo, task_repo, project_id, task_id)
}

#[tokio::test]
async fn test_start_ai_review_success() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);
    let review = service
        .start_ai_review(&task_id, &project_id)
        .await
        .unwrap();
    assert_eq!(review.task_id, task_id);
    assert_eq!(review.reviewer_type, ReviewerType::Ai);
    assert!(review.is_pending());
}

#[tokio::test]
async fn test_start_ai_review_disabled() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service =
        ReviewService::with_settings(review_repo, task_repo, ReviewSettings::ai_disabled());
    let result = service.start_ai_review(&task_id, &project_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_start_ai_review_already_pending() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);
    service
        .start_ai_review(&task_id, &project_id)
        .await
        .unwrap();
    let result = service.start_ai_review(&task_id, &project_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_process_review_approved() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);
    let mut review = service
        .start_ai_review(&task_id, &project_id)
        .await
        .unwrap();
    let input = CompleteReviewInput::approved("All tests pass");
    let result = service
        .process_review_result(&mut review, &input)
        .await
        .unwrap();
    assert!(result.is_none());
    assert!(review.is_approved());
}

#[tokio::test]
async fn test_process_review_needs_changes_creates_fix_task() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo.clone());
    let mut review = service
        .start_ai_review(&task_id, &project_id)
        .await
        .unwrap();
    let issue = ReviewIssueInput::new("Missing error handling", IssueSeverity::Major)
        .with_no_step_reason("General code quality");
    let input = CompleteReviewInput::needs_changes_with_issues(
        "Missing error handling",
        "Add try-catch",
        vec![issue],
    );
    let result = service
        .process_review_result(&mut review, &input)
        .await
        .unwrap();
    assert!(result.is_some());
    let fix_task_id = result.unwrap();
    let fix_task = task_repo.get_by_id(&fix_task_id).await.unwrap().unwrap();
    assert!(fix_task.title.starts_with("Fix:"));
    assert_eq!(fix_task.category, TaskCategory::Regular);
}

#[tokio::test]
async fn test_process_review_escalate() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);
    let mut review = service
        .start_ai_review(&task_id, &project_id)
        .await
        .unwrap();
    let input = CompleteReviewInput::escalate("Security concern", "Needs human review");
    let result = service
        .process_review_result(&mut review, &input)
        .await
        .unwrap();
    assert!(result.is_none());
    assert_eq!(review.status, ReviewStatus::Rejected);
}

#[tokio::test]
async fn test_fix_task_requires_approval_when_configured() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::with_settings(
        review_repo,
        task_repo.clone(),
        ReviewSettings::with_fix_approval(),
    );
    let fix_task = service
        .create_fix_task(&task_id, &project_id, "Fix the bug")
        .await
        .unwrap();
    assert_eq!(fix_task.internal_status, InternalStatus::Blocked);
}

// ========================================
// Fix Task Workflow Tests
// ========================================

#[tokio::test]
async fn test_approve_fix_task_success() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::with_settings(
        review_repo,
        task_repo.clone(),
        ReviewSettings::with_fix_approval(),
    );

    // Create a fix task (will be Blocked due to with_fix_approval)
    let fix_task = service
        .create_fix_task(&task_id, &project_id, "Fix the bug")
        .await
        .unwrap();
    assert_eq!(fix_task.internal_status, InternalStatus::Blocked);

    // Approve it
    service.approve_fix_task(&fix_task.id).await.unwrap();

    // Verify it's now Ready
    let updated = task_repo.get_by_id(&fix_task.id).await.unwrap().unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Ready);

    let history = task_repo.get_status_history(&fix_task.id).await.unwrap();
    assert!(history.iter().any(|entry| {
        entry.from == InternalStatus::Blocked
            && entry.to == InternalStatus::Ready
            && entry.trigger == "review_fix"
    }));
}

#[tokio::test]
async fn test_approve_fix_task_not_blocked_fails() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    // Use default settings (fix tasks are Ready, not Blocked)
    let service = ReviewService::new(review_repo, task_repo.clone());

    // Create a fix task (will be Ready)
    let fix_task = service
        .create_fix_task(&task_id, &project_id, "Fix the bug")
        .await
        .unwrap();
    assert_eq!(fix_task.internal_status, InternalStatus::Ready);

    // Trying to approve should fail
    let result = service.approve_fix_task(&fix_task.id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_approve_fix_task_not_found() {
    let (review_repo, task_repo, _project_id, _task_id) = setup();
    let service = ReviewService::new(review_repo, task_repo);

    let nonexistent_id = TaskId::from_string("nonexistent".to_string());
    let result = service.approve_fix_task(&nonexistent_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_reject_fix_task_creates_new_fix() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::with_settings(
        review_repo.clone(),
        task_repo.clone(),
        ReviewSettings::with_fix_approval(),
    );

    // Create a review and fix task
    let mut review = service
        .start_ai_review(&task_id, &project_id)
        .await
        .unwrap();
    let issue = ReviewIssueInput::new("Missing tests", IssueSeverity::Major)
        .with_no_step_reason("General requirement");
    let input =
        CompleteReviewInput::needs_changes_with_issues("Missing tests", "Add tests", vec![issue]);
    let fix_task_id = service
        .process_review_result(&mut review, &input)
        .await
        .unwrap()
        .unwrap();

    // Reject the fix task
    let new_fix_id = service
        .reject_fix_task(&fix_task_id, "Not good enough", &task_id)
        .await
        .unwrap();

    // Should have created a new fix task
    assert!(new_fix_id.is_some());
    let new_fix = task_repo
        .get_by_id(&new_fix_id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(new_fix.title.starts_with("Fix:"));
    assert!(new_fix
        .description
        .as_ref()
        .unwrap()
        .contains("Not good enough"));

    // Original fix task should be Failed
    let old_fix = task_repo.get_by_id(&fix_task_id).await.unwrap().unwrap();
    assert_eq!(old_fix.internal_status, InternalStatus::Failed);

    let fix_history = task_repo.get_status_history(&fix_task_id).await.unwrap();
    assert!(fix_history
        .iter()
        .any(|entry| { entry.to == InternalStatus::Failed && entry.trigger == "review_fix" }));
}

#[tokio::test]
async fn test_reject_fix_task_max_attempts_moves_to_backlog() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    // Set max_fix_attempts to 1
    let settings = ReviewSettings::with_max_attempts(1);
    let service = ReviewService::with_settings(review_repo.clone(), task_repo.clone(), settings);

    // Create a review and fix task
    let mut review = service
        .start_ai_review(&task_id, &project_id)
        .await
        .unwrap();
    let issue = ReviewIssueInput::new("Missing tests", IssueSeverity::Major)
        .with_no_step_reason("General requirement");
    let input =
        CompleteReviewInput::needs_changes_with_issues("Missing tests", "Add tests", vec![issue]);
    let fix_task_id = service
        .process_review_result(&mut review, &input)
        .await
        .unwrap()
        .unwrap();

    let mut original_for_backlog = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    original_for_backlog.internal_status = InternalStatus::PendingReview;
    task_repo.update(&original_for_backlog).await.unwrap();

    // At this point we have 1 fix action, which equals max_fix_attempts
    // Reject should move original to backlog
    let new_fix_id = service
        .reject_fix_task(&fix_task_id, "Still not good", &task_id)
        .await
        .unwrap();

    // Should NOT have created a new fix task
    assert!(new_fix_id.is_none());

    // Original task should be in Backlog
    let original = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(original.internal_status, InternalStatus::Backlog);

    let fix_history = task_repo.get_status_history(&fix_task_id).await.unwrap();
    assert!(fix_history
        .iter()
        .any(|entry| { entry.to == InternalStatus::Failed && entry.trigger == "review_fix" }));

    let original_history = task_repo.get_status_history(&task_id).await.unwrap();
    assert!(original_history
        .iter()
        .any(|entry| { entry.to == InternalStatus::Backlog && entry.trigger == "review_fix" }));
}

#[tokio::test]
async fn test_get_fix_attempt_count() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo.clone());

    // Initially 0
    assert_eq!(service.get_fix_attempt_count(&task_id).await.unwrap(), 0);

    // Create a review and add a fix task
    let mut review = service
        .start_ai_review(&task_id, &project_id)
        .await
        .unwrap();
    let issue = ReviewIssueInput::new("Missing tests", IssueSeverity::Major)
        .with_no_step_reason("General requirement");
    let input =
        CompleteReviewInput::needs_changes_with_issues("Missing tests", "Add tests", vec![issue]);
    service
        .process_review_result(&mut review, &input)
        .await
        .unwrap();

    // Now should be 1
    assert_eq!(service.get_fix_attempt_count(&task_id).await.unwrap(), 1);
}

#[tokio::test]
async fn test_move_to_backlog() {
    let (review_repo, task_repo, _project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo.clone());

    // Update task status to something other than Backlog
    let mut task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    task.internal_status = InternalStatus::PendingReview;
    task_repo.update(&task).await.unwrap();

    // Move to backlog
    service
        .move_to_backlog(&task_id, "Too complex to fix automatically")
        .await
        .unwrap();

    // Verify it's in Backlog
    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Backlog);

    let history = task_repo.get_status_history(&task_id).await.unwrap();
    assert!(history.iter().any(|entry| {
        entry.from == InternalStatus::PendingReview
            && entry.to == InternalStatus::Backlog
            && entry.trigger == "review_service"
    }));
}

// ========================================
// Human Review Tests
// ========================================

#[tokio::test]
async fn test_start_human_review_success() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);

    let review = service
        .start_human_review(&task_id, &project_id)
        .await
        .unwrap();

    assert_eq!(review.task_id, task_id);
    assert_eq!(review.reviewer_type, ReviewerType::Human);
    assert!(review.is_pending());
}

#[tokio::test]
async fn test_start_human_review_already_pending() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);

    // Start first human review
    service
        .start_human_review(&task_id, &project_id)
        .await
        .unwrap();

    // Trying to start another should fail
    let result = service.start_human_review(&task_id, &project_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_start_human_review_task_not_found() {
    let (review_repo, task_repo, project_id, _task_id) = setup();
    let service = ReviewService::new(review_repo, task_repo);

    let nonexistent_id = TaskId::from_string("nonexistent".to_string());
    let result = service
        .start_human_review(&nonexistent_id, &project_id)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_approve_human_review_success() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);

    // Start a human review
    let review = service
        .start_human_review(&task_id, &project_id)
        .await
        .unwrap();

    // Approve it
    service
        .approve_human_review(&review.id, Some("Looks good!".to_string()))
        .await
        .unwrap();

    // Verify review is approved
    let updated = review_repo.get_by_id(&review.id).await.unwrap().unwrap();
    assert!(updated.is_approved());
    assert_eq!(updated.notes, Some("Looks good!".to_string()));
}

#[tokio::test]
async fn test_approve_human_review_without_notes() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);

    let review = service
        .start_human_review(&task_id, &project_id)
        .await
        .unwrap();
    service
        .approve_human_review(&review.id, None)
        .await
        .unwrap();

    let updated = review_repo.get_by_id(&review.id).await.unwrap().unwrap();
    assert!(updated.is_approved());
}

#[tokio::test]
async fn test_approve_human_review_not_pending() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);

    // Start and approve a review
    let review = service
        .start_human_review(&task_id, &project_id)
        .await
        .unwrap();
    service
        .approve_human_review(&review.id, None)
        .await
        .unwrap();

    // Trying to approve again should fail
    let result = service.approve_human_review(&review.id, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_approve_human_review_not_found() {
    let (review_repo, task_repo, _project_id, _task_id) = setup();
    let service = ReviewService::new(review_repo, task_repo);

    let nonexistent_id = ReviewId::from_string("nonexistent");
    let result = service.approve_human_review(&nonexistent_id, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_request_changes_without_fix() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);

    let review = service
        .start_human_review(&task_id, &project_id)
        .await
        .unwrap();
    let result = service
        .request_changes(&review.id, "Missing tests".to_string(), None)
        .await
        .unwrap();

    // Should not create a fix task
    assert!(result.is_none());

    // Review should be changes_requested
    let updated = review_repo.get_by_id(&review.id).await.unwrap().unwrap();
    assert_eq!(updated.status, ReviewStatus::ChangesRequested);
    assert_eq!(updated.notes, Some("Missing tests".to_string()));
}

#[tokio::test]
async fn test_request_changes_with_fix() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo.clone());

    let review = service
        .start_human_review(&task_id, &project_id)
        .await
        .unwrap();
    let result = service
        .request_changes(
            &review.id,
            "Missing tests".to_string(),
            Some("Add unit tests for validation".to_string()),
        )
        .await
        .unwrap();

    // Should have created a fix task
    assert!(result.is_some());
    let fix_task_id = result.unwrap();

    // Verify fix task was created
    let fix_task = task_repo.get_by_id(&fix_task_id).await.unwrap().unwrap();
    assert!(fix_task.title.starts_with("Fix:"));
    assert_eq!(fix_task.category, TaskCategory::Regular);
    assert!(fix_task
        .description
        .as_ref()
        .unwrap()
        .contains("Add unit tests"));
}

#[tokio::test]
async fn test_request_changes_not_pending() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);

    // Start and approve a review
    let review = service
        .start_human_review(&task_id, &project_id)
        .await
        .unwrap();
    service
        .approve_human_review(&review.id, None)
        .await
        .unwrap();

    // Trying to request changes should fail
    let result = service
        .request_changes(&review.id, "Changes needed".to_string(), None)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_reject_human_review_success() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo.clone());

    let review = service
        .start_human_review(&task_id, &project_id)
        .await
        .unwrap();
    service
        .reject_human_review(&review.id, "Fundamentally wrong approach".to_string())
        .await
        .unwrap();

    // Verify review is rejected
    let updated_review = review_repo.get_by_id(&review.id).await.unwrap().unwrap();
    assert_eq!(updated_review.status, ReviewStatus::Rejected);
    assert_eq!(
        updated_review.notes,
        Some("Fundamentally wrong approach".to_string())
    );

    // Verify task is Failed
    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(updated_task.internal_status, InternalStatus::Failed);
}

#[tokio::test]
async fn test_reject_human_review_not_pending() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);

    // Start and reject a review
    let review = service
        .start_human_review(&task_id, &project_id)
        .await
        .unwrap();
    service
        .reject_human_review(&review.id, "Bad approach".to_string())
        .await
        .unwrap();

    // Trying to reject again should fail
    let result = service
        .reject_human_review(&review.id, "Still bad".to_string())
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_reject_human_review_not_found() {
    let (review_repo, task_repo, _project_id, _task_id) = setup();
    let service = ReviewService::new(review_repo, task_repo);

    let nonexistent_id = ReviewId::from_string("nonexistent");
    let result = service
        .reject_human_review(&nonexistent_id, "Rejected".to_string())
        .await;
    assert!(result.is_err());
}

// ========================================
// ReviewerType Regression Tests
// ========================================

/// Regression guard: complete_review(escalate) via the AI review path MUST produce ReviewerType::Ai.
/// This is a deliberate AI decision — do NOT change to System.
#[tokio::test]
async fn test_process_review_escalate_produces_reviewer_type_ai() {
    let (review_repo, task_repo, project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo);

    let mut review = service
        .start_ai_review(&task_id, &project_id)
        .await
        .unwrap();

    let input = CompleteReviewInput::escalate(
        "Security-sensitive change requires human oversight",
        "Please review authentication logic manually",
    );
    service
        .process_review_result(&mut review, &input)
        .await
        .unwrap();

    // The escalation via complete_review MUST be attributed to Ai reviewer.
    let notes = review_repo.get_notes_by_task_id(&task_id).await.unwrap();
    assert_eq!(notes.len(), 1, "Expected exactly one review note");
    assert_eq!(
        notes[0].reviewer,
        ReviewerType::Ai,
        "complete_review(escalate) must produce ReviewerType::Ai, not System"
    );
}

/// System escalation (move_to_backlog) MUST produce ReviewerType::System.
/// This path is triggered by policy limits, not an AI decision.
#[tokio::test]
async fn test_move_to_backlog_produces_reviewer_type_system() {
    let (review_repo, task_repo, _project_id, task_id) = setup();
    let service = ReviewService::new(review_repo.clone(), task_repo.clone());

    // Update task to non-backlog status first
    let mut task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    task.internal_status = InternalStatus::PendingReview;
    task_repo.update(&task).await.unwrap();

    service
        .move_to_backlog(&task_id, "Max retries exceeded by system policy")
        .await
        .unwrap();

    let notes = review_repo.get_notes_by_task_id(&task_id).await.unwrap();
    assert_eq!(notes.len(), 1, "Expected exactly one review note");
    assert_eq!(
        notes[0].reviewer,
        ReviewerType::System,
        "move_to_backlog (system policy) must produce ReviewerType::System, not Ai"
    );
}
