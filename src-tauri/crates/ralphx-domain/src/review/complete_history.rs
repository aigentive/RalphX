use crate::entities::{
    ProjectId, Review, ReviewNote, ReviewOutcome, ReviewStatus, ReviewerType, TaskId,
};

pub fn count_revision_cycles(notes: &[ReviewNote]) -> u32 {
    notes.iter()
        .filter(|note| note.outcome == ReviewOutcome::ChangesRequested)
        .count() as u32
}

pub fn pending_review_or_new(
    reviews: Vec<Review>,
    project_id: ProjectId,
    task_id: TaskId,
) -> (bool, Review) {
    match reviews
        .into_iter()
        .find(|review| review.status == ReviewStatus::Pending)
    {
        Some(review) => (false, review),
        None => (true, Review::new(project_id, task_id, ReviewerType::Ai)),
    }
}

pub fn build_ai_review_note(
    task_id: TaskId,
    outcome: ReviewOutcome,
    summary: Option<String>,
    notes: Option<String>,
    issues: Option<Vec<crate::entities::ReviewIssue>>,
    followup_session_id: Option<String>,
) -> ReviewNote {
    let mut review_note =
        ReviewNote::with_content(task_id, ReviewerType::Ai, outcome, summary, notes, issues);
    review_note.followup_session_id = followup_session_id;
    review_note
}

#[cfg(test)]
#[path = "complete_history_tests.rs"]
mod tests;
