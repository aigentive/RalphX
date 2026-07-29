use crate::entities::{ProjectId, Review, ReviewOutcome, ReviewStatus, ReviewerType, TaskId};

use super::{build_ai_review_note, count_revision_cycles, pending_review_or_new};

#[test]
fn count_revision_cycles_counts_changes_requested_notes() {
    let task_id = TaskId::new();
    let notes = vec![
        build_ai_review_note(
            task_id.clone(),
            ReviewOutcome::Approved,
            None,
            None,
            None,
            None,
        ),
        build_ai_review_note(
            task_id.clone(),
            ReviewOutcome::ChangesRequested,
            None,
            None,
            None,
            None,
        ),
        build_ai_review_note(
            task_id,
            ReviewOutcome::ChangesRequested,
            None,
            None,
            None,
            None,
        ),
    ];

    assert_eq!(count_revision_cycles(&notes), 2);
}

#[test]
fn pending_review_or_new_reuses_pending_review_when_present() {
    let project_id = ProjectId::new();
    let task_id = TaskId::new();
    let pending = Review::new(project_id.clone(), task_id.clone(), ReviewerType::Ai);
    let mut approved = Review::new(project_id.clone(), task_id.clone(), ReviewerType::Ai);
    approved.status = ReviewStatus::Approved;

    let (is_new, review) = pending_review_or_new(vec![approved, pending.clone()], project_id, task_id);

    assert!(!is_new);
    assert_eq!(review.id, pending.id);
}

#[test]
fn pending_review_or_new_creates_ai_review_when_missing() {
    let project_id = ProjectId::new();
    let task_id = TaskId::new();

    let (is_new, review) = pending_review_or_new(Vec::new(), project_id.clone(), task_id.clone());

    assert!(is_new);
    assert_eq!(review.project_id, project_id);
    assert_eq!(review.task_id, task_id);
    assert_eq!(review.reviewer_type, ReviewerType::Ai);
    assert_eq!(review.status, ReviewStatus::Pending);
}

#[test]
fn build_ai_review_note_sets_followup_link() {
    let task_id = TaskId::new();
    let note = build_ai_review_note(
        task_id.clone(),
        ReviewOutcome::Rejected,
        Some("summary".to_string()),
        Some("notes".to_string()),
        None,
        Some("session-1".to_string()),
    );

    assert_eq!(note.task_id, task_id);
    assert_eq!(note.reviewer, ReviewerType::Ai);
    assert_eq!(note.outcome, ReviewOutcome::Rejected);
    assert_eq!(note.summary.as_deref(), Some("summary"));
    assert_eq!(note.notes.as_deref(), Some("notes"));
    assert_eq!(note.followup_session_id.as_deref(), Some("session-1"));
}
