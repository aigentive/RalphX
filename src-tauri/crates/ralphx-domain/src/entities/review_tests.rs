use super::*;

// ===== ReviewId Tests =====

#[test]
fn test_review_id_new_generates_valid_uuid() {
    let id = ReviewId::new();
    assert_eq!(id.as_str().len(), 36);
    assert!(uuid::Uuid::parse_str(id.as_str()).is_ok());
}

#[test]
fn test_review_id_from_string() {
    let id = ReviewId::from_string("rev-123");
    assert_eq!(id.as_str(), "rev-123");
}

#[test]
fn test_review_id_equality() {
    let id1 = ReviewId::from_string("rev-abc");
    let id2 = ReviewId::from_string("rev-abc");
    let id3 = ReviewId::from_string("rev-xyz");
    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn test_review_id_serialization() {
    let id = ReviewId::from_string("rev-serialize");
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"rev-serialize\"");
    let parsed: ReviewId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, parsed);
}

// ===== ReviewActionId Tests =====

#[test]
fn test_review_action_id_new_generates_valid_uuid() {
    let id = ReviewActionId::new();
    assert_eq!(id.as_str().len(), 36);
    assert!(uuid::Uuid::parse_str(id.as_str()).is_ok());
}

#[test]
fn test_review_action_id_from_string() {
    let id = ReviewActionId::from_string("action-123");
    assert_eq!(id.as_str(), "action-123");
}

// ===== ReviewerType Tests =====

#[test]
fn test_reviewer_type_display() {
    assert_eq!(format!("{}", ReviewerType::Ai), "ai");
    assert_eq!(format!("{}", ReviewerType::Human), "human");
    assert_eq!(format!("{}", ReviewerType::System), "system");
}

#[test]
fn test_reviewer_type_from_str() {
    assert_eq!(ReviewerType::from_str("ai").unwrap(), ReviewerType::Ai);
    assert_eq!(ReviewerType::from_str("AI").unwrap(), ReviewerType::Ai);
    assert_eq!(
        ReviewerType::from_str("human").unwrap(),
        ReviewerType::Human
    );
    assert_eq!(
        ReviewerType::from_str("HUMAN").unwrap(),
        ReviewerType::Human
    );
    assert_eq!(
        ReviewerType::from_str("system").unwrap(),
        ReviewerType::System
    );
    assert_eq!(
        ReviewerType::from_str("SYSTEM").unwrap(),
        ReviewerType::System
    );
    assert!(ReviewerType::from_str("invalid").is_err());
}

#[test]
fn test_reviewer_type_serialization() {
    let ai = ReviewerType::Ai;
    let json = serde_json::to_string(&ai).unwrap();
    assert_eq!(json, "\"ai\"");
    let parsed: ReviewerType = serde_json::from_str(&json).unwrap();
    assert_eq!(ai, parsed);
}

// ===== ReviewStatus Tests =====

#[test]
fn test_review_status_display() {
    assert_eq!(format!("{}", ReviewStatus::Pending), "pending");
    assert_eq!(format!("{}", ReviewStatus::Approved), "approved");
    assert_eq!(
        format!("{}", ReviewStatus::ChangesRequested),
        "changes_requested"
    );
    assert_eq!(format!("{}", ReviewStatus::Rejected), "rejected");
}

#[test]
fn test_review_status_from_str() {
    assert_eq!(
        ReviewStatus::from_str("pending").unwrap(),
        ReviewStatus::Pending
    );
    assert_eq!(
        ReviewStatus::from_str("approved").unwrap(),
        ReviewStatus::Approved
    );
    assert_eq!(
        ReviewStatus::from_str("changes_requested").unwrap(),
        ReviewStatus::ChangesRequested
    );
    assert_eq!(
        ReviewStatus::from_str("rejected").unwrap(),
        ReviewStatus::Rejected
    );
    assert!(ReviewStatus::from_str("invalid").is_err());
}

#[test]
fn test_review_status_serialization() {
    let status = ReviewStatus::ChangesRequested;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, "\"changes_requested\"");
    let parsed: ReviewStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(status, parsed);
}

// ===== ReviewActionType Tests =====

#[test]
fn test_review_action_type_display() {
    assert_eq!(
        format!("{}", ReviewActionType::CreatedFixTask),
        "created_fix_task"
    );
    assert_eq!(
        format!("{}", ReviewActionType::MovedToBacklog),
        "moved_to_backlog"
    );
    assert_eq!(format!("{}", ReviewActionType::Approved), "approved");
}

#[test]
fn test_review_action_type_from_str() {
    assert_eq!(
        ReviewActionType::from_str("created_fix_task").unwrap(),
        ReviewActionType::CreatedFixTask
    );
    assert_eq!(
        ReviewActionType::from_str("moved_to_backlog").unwrap(),
        ReviewActionType::MovedToBacklog
    );
    assert_eq!(
        ReviewActionType::from_str("approved").unwrap(),
        ReviewActionType::Approved
    );
    assert!(ReviewActionType::from_str("invalid").is_err());
}

#[test]
fn test_review_action_type_serialization() {
    let action_type = ReviewActionType::CreatedFixTask;
    let json = serde_json::to_string(&action_type).unwrap();
    assert_eq!(json, "\"created_fix_task\"");
    let parsed: ReviewActionType = serde_json::from_str(&json).unwrap();
    assert_eq!(action_type, parsed);
}

// ===== Review Tests =====

#[test]
fn test_review_new() {
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());
    let review = Review::new(project_id.clone(), task_id.clone(), ReviewerType::Ai);

    assert_eq!(review.project_id, project_id);
    assert_eq!(review.task_id, task_id);
    assert_eq!(review.reviewer_type, ReviewerType::Ai);
    assert_eq!(review.status, ReviewStatus::Pending);
    assert!(review.notes.is_none());
    assert!(review.completed_at.is_none());
    assert!(review.is_pending());
    assert!(!review.is_complete());
}

#[test]
fn test_review_with_id() {
    let id = ReviewId::from_string("rev-custom");
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());
    let review = Review::with_id(id.clone(), project_id, task_id, ReviewerType::Human);

    assert_eq!(review.id, id);
}

#[test]
fn test_review_approve() {
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());
    let mut review = Review::new(project_id, task_id, ReviewerType::Ai);

    review.approve(Some("Looks good!".to_string()));

    assert!(review.is_approved());
    assert!(review.is_complete());
    assert!(!review.is_pending());
    assert_eq!(review.notes, Some("Looks good!".to_string()));
    assert!(review.completed_at.is_some());
}

#[test]
fn test_review_request_changes() {
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());
    let mut review = Review::new(project_id, task_id, ReviewerType::Ai);

    review.request_changes("Missing tests".to_string());

    assert_eq!(review.status, ReviewStatus::ChangesRequested);
    assert!(review.is_complete());
    assert!(!review.is_approved());
    assert_eq!(review.notes, Some("Missing tests".to_string()));
    assert!(review.completed_at.is_some());
}

#[test]
fn test_review_reject() {
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());
    let mut review = Review::new(project_id, task_id, ReviewerType::Human);

    review.reject("Fundamentally wrong approach".to_string());

    assert_eq!(review.status, ReviewStatus::Rejected);
    assert!(review.is_complete());
    assert!(!review.is_approved());
    assert_eq!(
        review.notes,
        Some("Fundamentally wrong approach".to_string())
    );
    assert!(review.completed_at.is_some());
}

#[test]
fn test_review_serialization() {
    let project_id = ProjectId::from_string("proj-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());
    let review = Review::new(project_id, task_id, ReviewerType::Ai);

    let json = serde_json::to_string(&review).unwrap();
    let parsed: Review = serde_json::from_str(&json).unwrap();

    assert_eq!(review.id, parsed.id);
    assert_eq!(review.project_id, parsed.project_id);
    assert_eq!(review.task_id, parsed.task_id);
    assert_eq!(review.reviewer_type, parsed.reviewer_type);
    assert_eq!(review.status, parsed.status);
}

// ===== ReviewAction Tests =====

#[test]
fn test_review_action_new() {
    let review_id = ReviewId::from_string("rev-1");
    let action = ReviewAction::new(review_id.clone(), ReviewActionType::Approved);

    assert_eq!(action.review_id, review_id);
    assert_eq!(action.action_type, ReviewActionType::Approved);
    assert!(action.target_task_id.is_none());
    assert!(!action.is_fix_task_action());
}

#[test]
fn test_review_action_with_target_task() {
    let review_id = ReviewId::from_string("rev-1");
    let target_task = TaskId::from_string("fix-task-1".to_string());
    let action = ReviewAction::with_target_task(
        review_id.clone(),
        ReviewActionType::CreatedFixTask,
        target_task.clone(),
    );

    assert_eq!(action.review_id, review_id);
    assert_eq!(action.action_type, ReviewActionType::CreatedFixTask);
    assert_eq!(action.target_task_id, Some(target_task));
    assert!(action.is_fix_task_action());
}

#[test]
fn test_review_action_with_id() {
    let id = ReviewActionId::from_string("action-custom");
    let review_id = ReviewId::from_string("rev-1");
    let action = ReviewAction::with_id(id.clone(), review_id, ReviewActionType::Approved);

    assert_eq!(action.id, id);
}

#[test]
fn test_review_action_serialization() {
    let review_id = ReviewId::from_string("rev-1");
    let action = ReviewAction::new(review_id, ReviewActionType::MovedToBacklog);

    let json = serde_json::to_string(&action).unwrap();
    let parsed: ReviewAction = serde_json::from_str(&json).unwrap();

    assert_eq!(action.id, parsed.id);
    assert_eq!(action.review_id, parsed.review_id);
    assert_eq!(action.action_type, parsed.action_type);
}

// ===== ReviewNoteId Tests =====

#[test]
fn test_review_note_id_new_generates_valid_uuid() {
    let id = ReviewNoteId::new();
    assert_eq!(id.as_str().len(), 36);
    assert!(uuid::Uuid::parse_str(id.as_str()).is_ok());
}

#[test]
fn test_review_note_id_from_string() {
    let id = ReviewNoteId::from_string("note-123");
    assert_eq!(id.as_str(), "note-123");
}

#[test]
fn test_review_note_id_equality() {
    let id1 = ReviewNoteId::from_string("note-abc");
    let id2 = ReviewNoteId::from_string("note-abc");
    let id3 = ReviewNoteId::from_string("note-xyz");
    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn test_review_note_id_serialization() {
    let id = ReviewNoteId::from_string("note-serialize");
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"note-serialize\"");
    let parsed: ReviewNoteId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, parsed);
}

// ===== ReviewOutcome Tests =====

#[test]
fn test_review_outcome_display() {
    assert_eq!(format!("{}", ReviewOutcome::Approved), "approved");
    assert_eq!(
        format!("{}", ReviewOutcome::ChangesRequested),
        "changes_requested"
    );
    assert_eq!(format!("{}", ReviewOutcome::Rejected), "rejected");
}

#[test]
fn test_review_outcome_from_str() {
    assert_eq!(
        ReviewOutcome::from_str("approved").unwrap(),
        ReviewOutcome::Approved
    );
    assert_eq!(
        ReviewOutcome::from_str("changes_requested").unwrap(),
        ReviewOutcome::ChangesRequested
    );
    assert_eq!(
        ReviewOutcome::from_str("rejected").unwrap(),
        ReviewOutcome::Rejected
    );
    assert!(ReviewOutcome::from_str("invalid").is_err());
}

#[test]
fn test_review_outcome_serialization() {
    let outcome = ReviewOutcome::ChangesRequested;
    let json = serde_json::to_string(&outcome).unwrap();
    assert_eq!(json, "\"changes_requested\"");
    let parsed: ReviewOutcome = serde_json::from_str(&json).unwrap();
    assert_eq!(outcome, parsed);
}

// ===== ReviewNote Tests =====

#[test]
fn test_review_note_new() {
    let task_id = TaskId::from_string("task-1".to_string());
    let note = ReviewNote::new(task_id.clone(), ReviewerType::Ai, ReviewOutcome::Approved);

    assert_eq!(note.task_id, task_id);
    assert_eq!(note.reviewer, ReviewerType::Ai);
    assert_eq!(note.outcome, ReviewOutcome::Approved);
    assert!(note.notes.is_none());
    assert!(note.is_positive());
    assert!(!note.is_negative());
}

#[test]
fn test_review_note_with_notes() {
    let task_id = TaskId::from_string("task-1".to_string());
    let note = ReviewNote::with_notes(
        task_id.clone(),
        ReviewerType::Human,
        ReviewOutcome::ChangesRequested,
        "Missing tests".to_string(),
    );

    assert_eq!(note.task_id, task_id);
    assert_eq!(note.reviewer, ReviewerType::Human);
    assert_eq!(note.outcome, ReviewOutcome::ChangesRequested);
    assert_eq!(note.notes, Some("Missing tests".to_string()));
    assert!(!note.is_positive());
    assert!(note.is_negative());
}

#[test]
fn test_review_note_with_id() {
    let id = ReviewNoteId::from_string("note-custom");
    let task_id = TaskId::from_string("task-1".to_string());
    let note = ReviewNote::with_id(
        id.clone(),
        task_id,
        ReviewerType::Ai,
        ReviewOutcome::Approved,
    );

    assert_eq!(note.id, id);
}

#[test]
fn test_review_note_is_positive() {
    let task_id = TaskId::from_string("task-1".to_string());

    let approved = ReviewNote::new(task_id.clone(), ReviewerType::Ai, ReviewOutcome::Approved);
    let changes = ReviewNote::new(
        task_id.clone(),
        ReviewerType::Ai,
        ReviewOutcome::ChangesRequested,
    );
    let rejected = ReviewNote::new(task_id, ReviewerType::Ai, ReviewOutcome::Rejected);

    assert!(approved.is_positive());
    assert!(!changes.is_positive());
    assert!(!rejected.is_positive());
}

#[test]
fn test_review_note_is_negative() {
    let task_id = TaskId::from_string("task-1".to_string());

    let approved = ReviewNote::new(task_id.clone(), ReviewerType::Ai, ReviewOutcome::Approved);
    let changes = ReviewNote::new(
        task_id.clone(),
        ReviewerType::Ai,
        ReviewOutcome::ChangesRequested,
    );
    let rejected = ReviewNote::new(task_id, ReviewerType::Ai, ReviewOutcome::Rejected);

    assert!(!approved.is_negative());
    assert!(changes.is_negative());
    assert!(rejected.is_negative());
}

#[test]
fn test_review_note_serialization() {
    let task_id = TaskId::from_string("task-1".to_string());
    let note = ReviewNote::with_notes(
        task_id,
        ReviewerType::Human,
        ReviewOutcome::Approved,
        "Looks good!".to_string(),
    );

    let json = serde_json::to_string(&note).unwrap();
    let parsed: ReviewNote = serde_json::from_str(&json).unwrap();

    assert_eq!(note.id, parsed.id);
    assert_eq!(note.task_id, parsed.task_id);
    assert_eq!(note.reviewer, parsed.reviewer);
    assert_eq!(note.outcome, parsed.outcome);
    assert_eq!(note.notes, parsed.notes);
}

// ===== ReviewOutcome::ApprovedNoChanges Tests =====

#[test]
fn test_review_outcome_approved_no_changes_display() {
    assert_eq!(
        format!("{}", ReviewOutcome::ApprovedNoChanges),
        "approved_no_changes"
    );
}

#[test]
fn test_review_outcome_approved_no_changes_from_str() {
    let result = ReviewOutcome::from_str("approved_no_changes");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ReviewOutcome::ApprovedNoChanges);
}

#[test]
fn test_review_outcome_approved_no_changes_from_str_case_insensitive() {
    // ReviewOutcome::from_str uses to_lowercase() so it IS case-insensitive
    let result = ReviewOutcome::from_str("APPROVED_NO_CHANGES");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ReviewOutcome::ApprovedNoChanges);
}

#[test]
fn test_review_outcome_approved_no_changes_serde_roundtrip() {
    let outcome = ReviewOutcome::ApprovedNoChanges;
    let json = serde_json::to_string(&outcome).unwrap();
    assert_eq!(json, "\"approved_no_changes\"");
    let parsed: ReviewOutcome = serde_json::from_str(&json).unwrap();
    assert_eq!(outcome, parsed);
}

#[test]
fn test_review_note_is_positive_for_approved_no_changes() {
    let task_id = TaskId::from_string("task-test".to_string());
    let note = ReviewNote::new(task_id, ReviewerType::Ai, ReviewOutcome::ApprovedNoChanges);
    assert!(note.is_positive(), "ApprovedNoChanges should be positive");
    assert!(
        !note.is_negative(),
        "ApprovedNoChanges should not be negative"
    );
}

#[test]
fn test_review_note_is_positive_still_true_for_approved() {
    let task_id = TaskId::from_string("task-test2".to_string());
    let note = ReviewNote::new(task_id, ReviewerType::Ai, ReviewOutcome::Approved);
    assert!(note.is_positive());
}

#[test]
fn test_review_note_is_negative_false_for_approved_no_changes() {
    let task_id = TaskId::from_string("task-test3".to_string());
    let note = ReviewNote::new(task_id, ReviewerType::Ai, ReviewOutcome::ApprovedNoChanges);
    assert!(!note.is_negative());
}
