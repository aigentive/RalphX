use crate::entities::{IssueSeverity, ReviewNoteId, TaskId, TaskStepId};

use super::{build_review_issue_entities, build_review_note_issues};
use crate::review::ParsedReviewIssue;

fn sample_parsed_issue() -> ParsedReviewIssue {
    ParsedReviewIssue {
        title: "Missing guard".to_string(),
        description: Some("Add the null guard".to_string()),
        severity: IssueSeverity::Major,
        category: None,
        step_id: Some(TaskStepId::from_string("step-1")),
        no_step_reason: None,
        file_path: Some("src/main.rs".to_string()),
        line_number: Some(42),
        code_snippet: Some("let value = maybe.unwrap();".to_string()),
    }
}

#[test]
fn build_review_note_issues_uses_description_fallbacks() {
    let issues = build_review_note_issues(&[
        sample_parsed_issue(),
        ParsedReviewIssue {
            description: None,
            ..sample_parsed_issue()
        },
    ]);

    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].severity, "major");
    assert_eq!(issues[0].file.as_deref(), Some("src/main.rs"));
    assert_eq!(issues[0].line, Some(42));
    assert_eq!(issues[0].description, "Add the null guard");
    assert_eq!(issues[1].description, "Missing guard");
}

#[test]
fn build_review_issue_entities_preserves_structured_issue_fields() {
    let review_note_id = ReviewNoteId::from_string("note-1");
    let task_id = TaskId::from_string("task-1".to_string());
    let entities =
        build_review_issue_entities(vec![sample_parsed_issue()], review_note_id.clone(), task_id.clone());

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.review_note_id, review_note_id);
    assert_eq!(entity.task_id, task_id);
    assert_eq!(entity.title, "Missing guard");
    assert_eq!(entity.description.as_deref(), Some("Add the null guard"));
    assert_eq!(entity.file_path.as_deref(), Some("src/main.rs"));
    assert_eq!(entity.line_number, Some(42));
    assert_eq!(entity.code_snippet.as_deref(), Some("let value = maybe.unwrap();"));
}
