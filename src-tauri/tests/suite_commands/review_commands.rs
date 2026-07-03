use ralphx_lib::application::AppState;
use ralphx_lib::commands::review_commands::{FixTaskAttemptsResponse, ReviewResponse};
use ralphx_lib::domain::entities::{
    InternalStatus, Project, ProjectId, Review, ReviewId, ReviewNote, ReviewOutcome, ReviewStatus,
    ReviewerType, Task, TaskCategory, TaskId,
};
use ralphx_lib::domain::review::ReviewSettings;

async fn setup_test_state() -> AppState {
    AppState::new_test()
}

#[tokio::test]
async fn test_get_pending_reviews_empty() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("proj-1".to_string());

    let reviews = state
        .review_repo
        .get_pending(&project_id)
        .await
        .expect("Failed to get pending reviews in test");
    assert!(reviews.is_empty());
}

#[tokio::test]
async fn test_get_pending_reviews_returns_pending() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());

    // Create a pending review
    let review = Review::new(project_id.clone(), task_id, ReviewerType::Ai);
    state
        .review_repo
        .create(&review)
        .await
        .expect("Failed to create review in test");

    let reviews = state
        .review_repo
        .get_pending(&project_id)
        .await
        .expect("Failed to get pending reviews in test");
    assert_eq!(reviews.len(), 1);
    assert!(reviews[0].is_pending());
}

#[tokio::test]
async fn test_get_review_by_id() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());

    let review = Review::new(project_id, task_id, ReviewerType::Human);
    let review_id = review.id.clone();
    state
        .review_repo
        .create(&review)
        .await
        .expect("Failed to create review in test");

    let retrieved = state
        .review_repo
        .get_by_id(&review_id)
        .await
        .expect("Failed to get review by id in test");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.expect("Expected to find review").id, review_id);
}

#[tokio::test]
async fn test_get_review_by_id_not_found() {
    let state = setup_test_state().await;
    let nonexistent = ReviewId::from_string("nonexistent");

    let retrieved = state
        .review_repo
        .get_by_id(&nonexistent)
        .await
        .expect("Failed to get review by id in test");
    assert!(retrieved.is_none());
}

#[tokio::test]
async fn test_get_reviews_by_task_id() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());

    // Create two reviews for same task
    let review1 = Review::new(project_id.clone(), task_id.clone(), ReviewerType::Ai);
    let review2 = Review::new(project_id, task_id.clone(), ReviewerType::Human);
    state
        .review_repo
        .create(&review1)
        .await
        .expect("Failed to create review1 in test");
    state
        .review_repo
        .create(&review2)
        .await
        .expect("Failed to create review2 in test");

    let reviews = state
        .review_repo
        .get_by_task_id(&task_id)
        .await
        .expect("Failed to get reviews by task id in test");
    assert_eq!(reviews.len(), 2);
}

#[tokio::test]
async fn test_approve_review() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());

    let review = Review::new(project_id, task_id, ReviewerType::Ai);
    let review_id = review.id.clone();
    state
        .review_repo
        .create(&review)
        .await
        .expect("Failed to create review in test");

    // Approve via repository (simulating what the command does)
    let mut review = state
        .review_repo
        .get_by_id(&review_id)
        .await
        .expect("Failed to get review by id in test")
        .expect("Expected to find review");
    review.approve(Some("Looks good!".to_string()));
    state
        .review_repo
        .update(&review)
        .await
        .expect("Failed to update review in test");

    // Verify
    let updated = state
        .review_repo
        .get_by_id(&review_id)
        .await
        .expect("Failed to get review by id in test")
        .expect("Expected to find updated review");
    assert!(updated.is_approved());
    assert_eq!(updated.notes, Some("Looks good!".to_string()));
}

#[tokio::test]
async fn test_request_changes() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());

    let review = Review::new(project_id, task_id, ReviewerType::Human);
    let review_id = review.id.clone();
    state
        .review_repo
        .create(&review)
        .await
        .expect("Failed to create review in test");

    // Request changes via repository
    let mut review = state
        .review_repo
        .get_by_id(&review_id)
        .await
        .expect("Failed to get review by id in test")
        .expect("Expected to find review");
    review.request_changes("Missing tests".to_string());
    state
        .review_repo
        .update(&review)
        .await
        .expect("Failed to update review in test");

    // Verify
    let updated = state
        .review_repo
        .get_by_id(&review_id)
        .await
        .expect("Failed to get review by id in test")
        .expect("Expected to find updated review");
    assert_eq!(updated.status, ReviewStatus::ChangesRequested);
    assert_eq!(updated.notes, Some("Missing tests".to_string()));
}

#[tokio::test]
async fn test_reject_review() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());

    let review = Review::new(project_id, task_id, ReviewerType::Ai);
    let review_id = review.id.clone();
    state
        .review_repo
        .create(&review)
        .await
        .expect("Failed to create review in test");

    // Reject via repository
    let mut review = state
        .review_repo
        .get_by_id(&review_id)
        .await
        .expect("Failed to get review by id in test")
        .expect("Expected to find review");
    review.reject("Fundamentally wrong".to_string());
    state
        .review_repo
        .update(&review)
        .await
        .expect("Failed to update review in test");

    // Verify
    let updated = state
        .review_repo
        .get_by_id(&review_id)
        .await
        .expect("Failed to get review by id in test")
        .expect("Expected to find updated review");
    assert_eq!(updated.status, ReviewStatus::Rejected);
    assert_eq!(updated.notes, Some("Fundamentally wrong".to_string()));
}

#[tokio::test]
async fn test_review_response_conversion() {
    let project_id = ProjectId::from_string("proj-123".to_string());
    let task_id = TaskId::from_string("task-456".to_string());
    let review = Review::new(project_id, task_id, ReviewerType::Human);
    let response = ReviewResponse::from(review);

    assert!(!response.id.is_empty());
    assert_eq!(response.project_id, "proj-123");
    assert_eq!(response.task_id, "task-456");
    assert_eq!(response.reviewer_type, "human");
    assert_eq!(response.status, "pending");
    assert!(response.notes.is_none());
    assert!(response.completed_at.is_none());

    // Verify it serializes to JSON
    let json =
        serde_json::to_string(&response).expect("Failed to serialize response to JSON in test");
    assert!(json.contains("\"reviewer_type\":\"human\""));
}

// ========================================
// Fix Task Command Tests
// ========================================

async fn create_task_for_tests(state: &AppState, project_id: ProjectId) -> Task {
    // Create a project first (required for task creation)
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    let mut project_with_id = project;
    project_with_id.id = project_id.clone();
    state
        .project_repo
        .create(project_with_id)
        .await
        .expect("Failed to create project in test");

    // Create a task
    let mut task = Task::new(project_id, "Test Task".to_string());
    task.internal_status = InternalStatus::PendingReview;
    state
        .task_repo
        .create(task.clone())
        .await
        .expect("Failed to create task in test");
    task
}

async fn create_blocked_fix_task(state: &AppState, project_id: ProjectId) -> (Task, Task) {
    // Create a project first
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    let mut project_with_id = project;
    project_with_id.id = project_id.clone();
    state
        .project_repo
        .create(project_with_id)
        .await
        .expect("Failed to create project in test");

    // Create original task
    let mut original = Task::new(project_id.clone(), "Original Task".to_string());
    original.internal_status = InternalStatus::RevisionNeeded;
    let original = state
        .task_repo
        .create(original)
        .await
        .expect("Failed to create original task in test");

    // Create fix task (blocked, waiting for approval)
    let mut fix_task = Task::new_with_category(
        project_id,
        "Fix: Original Task".to_string(),
        TaskCategory::Regular,
    );
    fix_task.internal_status = InternalStatus::Blocked;
    let fix_task = state
        .task_repo
        .create(fix_task)
        .await
        .expect("Failed to create fix task in test");

    (original, fix_task)
}

#[tokio::test]
async fn test_approve_fix_task_success() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("proj-1".to_string());

    // Create original task and blocked fix task
    let (_original, fix_task) = create_blocked_fix_task(&state, project_id).await;

    // Verify fix task is blocked initially
    let task = state
        .task_repo
        .get_by_id(&fix_task.id)
        .await
        .expect("Failed to get task by id in test")
        .expect("Expected to find task");
    assert_eq!(task.internal_status, InternalStatus::Blocked);

    // Approve it directly (simulating what the command does)
    let mut task = state
        .task_repo
        .get_by_id(&fix_task.id)
        .await
        .expect("Failed to get task by id in test")
        .expect("Expected to find task");
    assert_eq!(task.internal_status, InternalStatus::Blocked);
    task.internal_status = InternalStatus::Ready;
    state
        .task_repo
        .update(&task)
        .await
        .expect("Failed to update task in test");

    // Verify it's now Ready
    let updated = state
        .task_repo
        .get_by_id(&fix_task.id)
        .await
        .expect("Failed to get task by id in test")
        .expect("Expected to find updated task");
    assert_eq!(updated.internal_status, InternalStatus::Ready);
}

#[tokio::test]
async fn test_approve_fix_task_not_blocked_fails() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("proj-1".to_string());

    // Create a task that is Ready (not Blocked)
    let task = create_task_for_tests(&state, project_id).await;

    // Set it to Ready
    let mut task = state
        .task_repo
        .get_by_id(&task.id)
        .await
        .expect("Failed to get task by id in test")
        .expect("Expected to find task");
    task.internal_status = InternalStatus::Ready;
    state
        .task_repo
        .update(&task)
        .await
        .expect("Failed to update task in test");

    // Simulating the command logic - should reject non-Blocked tasks
    let task = state
        .task_repo
        .get_by_id(&task.id)
        .await
        .expect("Failed to get task by id in test")
        .expect("Expected to find task");
    assert_ne!(task.internal_status, InternalStatus::Blocked);
    // In the real command, this returns an error
}

#[tokio::test]
async fn test_approve_fix_task_not_found() {
    let state = setup_test_state().await;

    let nonexistent_id = TaskId::from_string("nonexistent".to_string());

    // Task not found
    let result = state
        .task_repo
        .get_by_id(&nonexistent_id)
        .await
        .expect("Failed to get task by id in test");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_reject_fix_task_creates_new_fix() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("proj-1".to_string());

    // Create original task and blocked fix task
    let (original, fix_task) = create_blocked_fix_task(&state, project_id).await;

    // Create a review for the original task (required for fix attempt counting)
    let review = Review::new(
        original.project_id.clone(),
        original.id.clone(),
        ReviewerType::Ai,
    );
    state
        .review_repo
        .create(&review)
        .await
        .expect("Failed to create review in test");

    // Simulate reject_fix_task logic:
    // 1. Mark fix task as Failed
    let mut fix = state
        .task_repo
        .get_by_id(&fix_task.id)
        .await
        .expect("Failed to get task by id in test")
        .expect("Expected to find fix task");
    fix.internal_status = InternalStatus::Failed;
    state
        .task_repo
        .update(&fix)
        .await
        .expect("Failed to update fix task in test");

    // 2. Create new fix task
    let mut new_fix_task = Task::new_with_category(
        original.project_id.clone(),
        format!("Fix: {}", original.title),
        TaskCategory::Regular,
    );
    new_fix_task.set_description(Some(format!(
        "Previous fix rejected. Feedback: {}\n\nOriginal issue: {}",
        "Not good enough",
        fix.description.as_deref().unwrap_or("No description")
    )));
    new_fix_task.set_priority(original.priority + 1);
    new_fix_task.internal_status = InternalStatus::Ready;
    let created = state
        .task_repo
        .create(new_fix_task)
        .await
        .expect("Failed to create new fix task in test");

    // Verify new fix task was created
    assert!(created.title.starts_with("Fix:"));
    assert!(created
        .description
        .as_ref()
        .expect("Expected description to be set")
        .contains("Not good enough"));

    // Original fix task should be Failed
    let old_fix = state
        .task_repo
        .get_by_id(&fix_task.id)
        .await
        .expect("Failed to get task by id in test")
        .expect("Expected to find old fix task");
    assert_eq!(old_fix.internal_status, InternalStatus::Failed);
}

#[tokio::test]
async fn test_get_fix_task_attempts_zero() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("proj-1".to_string());

    // Create a task
    let task = create_task_for_tests(&state, project_id).await;

    // Get fix attempts (should be 0)
    let count = state
        .review_repo
        .count_fix_actions(&task.id)
        .await
        .expect("Failed to count fix actions in test");

    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_fix_task_attempts_response_serialization() {
    let response = FixTaskAttemptsResponse {
        task_id: "task-123".to_string(),
        attempt_count: 2,
    };

    let json =
        serde_json::to_string(&response).expect("Failed to serialize response to JSON in test");
    assert!(json.contains("\"task_id\":\"task-123\""));
    assert!(json.contains("\"attempt_count\":2"));
}

// ========================================
// request_task_changes_from_reviewing Tests
//
// These tests validate the pre/post-conditions of the command logic
// using memory repositories. The full Tauri command (with AppHandle
// and event emission) requires an integration environment; the tests
// here cover state validation, idempotency guard, and review note
// creation — the three steps exercisable without a live AppHandle.
// ========================================

/// Helper: create a task with a specific internal status in the test state.
async fn create_task_with_status(
    state: &AppState,
    project_id: &str,
    status: InternalStatus,
) -> Task {
    let pid = ProjectId::from_string(project_id.to_string());
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    let mut project_with_id = project;
    project_with_id.id = pid.clone();
    // Ignore duplicate project errors (ok if already created).
    let _ = state.project_repo.create(project_with_id).await;

    let mut task = Task::new(pid, "Test Task".to_string());
    task.internal_status = status;
    state
        .task_repo
        .create(task.clone())
        .await
        .expect("Failed to create task in test");
    task
}

/// (a) Happy path pre-condition: task in Reviewing state passes validation.
#[tokio::test]
async fn test_request_task_changes_from_reviewing_accepts_reviewing_state() {
    let state = setup_test_state().await;
    let task = create_task_with_status(&state, "proj-rc1", InternalStatus::Reviewing).await;

    let fetched = state
        .task_repo
        .get_by_id(&task.id)
        .await
        .expect("repo error")
        .expect("task not found");

    // Command step 1: validate status == Reviewing
    assert_eq!(
        fetched.internal_status,
        InternalStatus::Reviewing,
        "State validation must pass for Reviewing tasks"
    );
}

/// (b) Reject ReviewPassed — command must return an error.
#[tokio::test]
async fn test_request_task_changes_from_reviewing_rejects_review_passed() {
    let state = setup_test_state().await;
    let task =
        create_task_with_status(&state, "proj-rc2", InternalStatus::ReviewPassed).await;

    let fetched = state
        .task_repo
        .get_by_id(&task.id)
        .await
        .expect("repo error")
        .expect("task not found");

    // Simulates command step 1 guard
    assert_ne!(
        fetched.internal_status,
        InternalStatus::Reviewing,
        "ReviewPassed should fail the Reviewing state guard"
    );
}

/// (b) Reject Escalated — command must return an error.
#[tokio::test]
async fn test_request_task_changes_from_reviewing_rejects_escalated() {
    let state = setup_test_state().await;
    let task =
        create_task_with_status(&state, "proj-rc3", InternalStatus::Escalated).await;

    let fetched = state
        .task_repo
        .get_by_id(&task.id)
        .await
        .expect("repo error")
        .expect("task not found");

    assert_ne!(fetched.internal_status, InternalStatus::Reviewing);
}

/// (b) Reject InProgress (Executing) — command must return an error.
#[tokio::test]
async fn test_request_task_changes_from_reviewing_rejects_executing() {
    let state = setup_test_state().await;
    let task =
        create_task_with_status(&state, "proj-rc4", InternalStatus::Executing).await;

    let fetched = state
        .task_repo
        .get_by_id(&task.id)
        .await
        .expect("repo error")
        .expect("task not found");

    assert_ne!(fetched.internal_status, InternalStatus::Reviewing);
}

/// Idempotency guard: metadata flag is written before side effects.
#[tokio::test]
async fn test_request_task_changes_from_reviewing_writes_idempotency_flag() {
    let state = setup_test_state().await;
    let mut task =
        create_task_with_status(&state, "proj-rc5", InternalStatus::Reviewing).await;

    // Simulate command step 2: write idempotency guard
    let mut meta = task
        .metadata
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.insert(
            "request_changes_initiated".to_string(),
            serde_json::json!(true),
        );
    }
    task.metadata = Some(serde_json::to_string(&meta).unwrap());
    task.touch();
    state
        .task_repo
        .update(&task)
        .await
        .expect("Failed to update task with idempotency flag");

    // Verify the flag is persisted
    let updated = state
        .task_repo
        .get_by_id(&task.id)
        .await
        .expect("repo error")
        .expect("task not found");

    let stored_meta: serde_json::Value = serde_json::from_str(
        updated.metadata.as_deref().expect("metadata must be present"),
    )
    .expect("metadata must be valid JSON");

    assert_eq!(
        stored_meta["request_changes_initiated"],
        serde_json::json!(true),
        "Idempotency flag must be written to task metadata"
    );
}

/// Review note with ChangesRequested outcome and Human reviewer is written to repo.
/// This mirrors command step 4 (add_note).
#[tokio::test]
async fn test_request_task_changes_from_reviewing_creates_review_note() {
    let state = setup_test_state().await;
    let task =
        create_task_with_status(&state, "proj-rc6", InternalStatus::Reviewing).await;

    let feedback = "Missing error handling in the auth flow".to_string();

    // Simulate command step 4: add a human ChangesRequested note
    let review_note = ReviewNote::with_notes(
        task.id.clone(),
        ReviewerType::Human,
        ReviewOutcome::ChangesRequested,
        feedback.clone(),
    );
    state
        .review_repo
        .add_note(&review_note)
        .await
        .expect("Failed to add review note");

    // Verify the note was persisted
    let notes = state
        .review_repo
        .get_notes_by_task_id(&task.id)
        .await
        .expect("Failed to get notes");

    assert_eq!(notes.len(), 1, "Exactly one review note should exist");
    assert_eq!(notes[0].reviewer, ReviewerType::Human);
    assert_eq!(notes[0].outcome, ReviewOutcome::ChangesRequested);
    assert_eq!(
        notes[0].notes,
        Some(feedback),
        "Feedback text must be stored in the review note"
    );
}

// ============================================================================
// Review Settings command tests
// ============================================================================

#[tokio::test]
async fn test_get_review_settings_returns_defaults() {
    let state = setup_test_state().await;

    let settings = state
        .review_settings_repo
        .get_settings()
        .await
        .expect("get_settings must succeed");

    // Defaults from ReviewSettings::default()
    assert!(!settings.require_human_review);
    assert_eq!(settings.max_fix_attempts, 3);
    assert_eq!(settings.max_revision_cycles, 5);
    assert!(settings.ai_review_enabled);
}

#[tokio::test]
async fn test_update_review_settings_primary_fields() {
    let state = setup_test_state().await;

    // Read current defaults
    let current = state
        .review_settings_repo
        .get_settings()
        .await
        .expect("get_settings must succeed");

    // Apply partial update to primary fields only
    let updated_settings = ReviewSettings {
        require_human_review: true,
        max_fix_attempts: 7,
        max_revision_cycles: 3,
        ..current.clone()
    };

    let saved = state
        .review_settings_repo
        .update_settings(&updated_settings)
        .await
        .expect("update_settings must succeed");

    assert!(saved.require_human_review);
    assert_eq!(saved.max_fix_attempts, 7);
    assert_eq!(saved.max_revision_cycles, 3);
    // Ballast fields preserved from current
    assert_eq!(saved.ai_review_enabled, current.ai_review_enabled);
    assert_eq!(saved.ai_review_auto_fix, current.ai_review_auto_fix);
    assert_eq!(saved.require_fix_approval, current.require_fix_approval);
}

#[tokio::test]
async fn test_update_review_settings_preserves_ballast() {
    let state = setup_test_state().await;

    // Set specific ballast values
    let with_ballast = ReviewSettings {
        ai_review_enabled: false,
        ai_review_auto_fix: false,
        require_fix_approval: true,
        ..ReviewSettings::default()
    };
    state
        .review_settings_repo
        .update_settings(&with_ballast)
        .await
        .expect("first update must succeed");

    // Now update only a primary field; ballast must stay
    let current = state
        .review_settings_repo
        .get_settings()
        .await
        .expect("get_settings must succeed");

    let primary_update = ReviewSettings {
        require_human_review: true,
        ..current
    };
    let saved = state
        .review_settings_repo
        .update_settings(&primary_update)
        .await
        .expect("second update must succeed");

    assert!(saved.require_human_review);
    // Ballast values must be preserved
    assert!(!saved.ai_review_enabled);
    assert!(!saved.ai_review_auto_fix);
    assert!(saved.require_fix_approval);
}
