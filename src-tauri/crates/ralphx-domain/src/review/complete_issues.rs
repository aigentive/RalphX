use crate::entities::{ReviewIssue, ReviewIssueEntity, ReviewNoteId, TaskId};

use super::ParsedReviewIssue;

pub fn build_review_note_issues(issues: &[ParsedReviewIssue]) -> Vec<ReviewIssue> {
    issues
        .iter()
        .map(|issue| ReviewIssue {
            severity: issue.severity.to_db_string().to_string(),
            file: issue.file_path.clone(),
            line: issue.line_number,
            description: issue
                .description
                .clone()
                .unwrap_or_else(|| issue.title.clone()),
        })
        .collect()
}

pub fn build_review_issue_entities(
    issues: Vec<ParsedReviewIssue>,
    review_note_id: ReviewNoteId,
    task_id: TaskId,
) -> Vec<ReviewIssueEntity> {
    issues
        .into_iter()
        .map(|issue| {
            let mut entity =
                ReviewIssueEntity::new(review_note_id.clone(), task_id.clone(), issue.title, issue.severity);
            entity.description = issue.description;
            entity.category = issue.category;
            entity.step_id = issue.step_id;
            entity.no_step_reason = issue.no_step_reason;
            entity.file_path = issue.file_path;
            entity.line_number = issue.line_number;
            entity.code_snippet = issue.code_snippet;
            entity
        })
        .collect()
}

#[cfg(test)]
#[path = "complete_issues_tests.rs"]
mod tests;
