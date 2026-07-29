use crate::entities::{InternalStatus, Review, ReviewOutcome};

use super::ReviewToolOutcome;

pub fn apply_review_outcome(
    review: &mut Review,
    outcome: ReviewToolOutcome,
    feedback: Option<String>,
) -> ReviewOutcome {
    match outcome {
        ReviewToolOutcome::Approved => {
            review.approve(feedback);
            ReviewOutcome::Approved
        }
        ReviewToolOutcome::ApprovedNoChanges => {
            review.approve(feedback);
            ReviewOutcome::ApprovedNoChanges
        }
        ReviewToolOutcome::NeedsChanges => {
            review.request_changes(feedback.unwrap_or_default());
            ReviewOutcome::ChangesRequested
        }
        ReviewToolOutcome::Escalate => {
            review.reject(feedback.unwrap_or_default());
            ReviewOutcome::Rejected
        }
    }
}

pub fn review_note_content(
    outcome: ReviewToolOutcome,
    feedback: Option<&str>,
    escalation_reason: Option<&str>,
) -> Option<String> {
    if matches!(outcome, ReviewToolOutcome::Escalate) {
        escalation_reason
            .map(str::to_string)
            .or_else(|| feedback.map(str::to_string))
    } else {
        feedback.map(str::to_string)
    }
}

pub fn complete_review_response_message(followup_session_id: Option<&str>) -> String {
    match followup_session_id {
        Some(session_id) => {
            format!("Review submitted successfully. Follow-up ideation session created: {session_id}")
        }
        None => "Review submitted successfully".to_string(),
    }
}

pub fn approved_target_status(require_human_review: bool) -> InternalStatus {
    if require_human_review {
        InternalStatus::ReviewPassed
    } else {
        InternalStatus::Approved
    }
}

pub fn approved_no_changes_target_status(require_human_review: bool) -> InternalStatus {
    if require_human_review {
        InternalStatus::ReviewPassed
    } else {
        InternalStatus::Merged
    }
}

#[cfg(test)]
#[path = "complete_result_tests.rs"]
mod tests;
