use super::*;
use crate::entities::{IssueSeverity, TaskStepId};
use std::str::FromStr;

#[test]
fn test_review_tool_outcome_display() {
    assert_eq!(format!("{}", ReviewToolOutcome::Approved), "approved");
    assert_eq!(
        format!("{}", ReviewToolOutcome::NeedsChanges),
        "needs_changes"
    );
    assert_eq!(format!("{}", ReviewToolOutcome::Escalate), "escalate");
}

#[test]
fn test_review_tool_outcome_from_str() {
    assert_eq!(
        ReviewToolOutcome::from_str("approved").unwrap(),
        ReviewToolOutcome::Approved
    );
    assert_eq!(
        ReviewToolOutcome::from_str("APPROVED").unwrap(),
        ReviewToolOutcome::Approved
    );
    assert_eq!(
        ReviewToolOutcome::from_str("needs_changes").unwrap(),
        ReviewToolOutcome::NeedsChanges
    );
    assert_eq!(
        ReviewToolOutcome::from_str("escalate").unwrap(),
        ReviewToolOutcome::Escalate
    );
}

#[test]
fn test_review_tool_outcome_from_str_invalid() {
    let result = ReviewToolOutcome::from_str("invalid");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.0, "invalid");
    assert!(err.to_string().contains("invalid review tool outcome"));
}

#[test]
fn test_review_tool_outcome_serialization() {
    let outcome = ReviewToolOutcome::NeedsChanges;
    let json = serde_json::to_string(&outcome).unwrap();
    assert_eq!(json, "\"needs_changes\"");

    let parsed: ReviewToolOutcome = serde_json::from_str(&json).unwrap();
    assert_eq!(outcome, parsed);
}

#[test]
fn test_complete_review_input_approved() {
    let input = CompleteReviewInput::approved("Looks good, all tests pass");

    assert_eq!(input.outcome, ReviewToolOutcome::Approved);
    assert_eq!(input.notes, "Looks good, all tests pass");
    assert!(input.fix_description.is_none());
    assert!(input.escalation_reason.is_none());
    assert!(input.is_approved());
    assert!(!input.is_needs_changes());
    assert!(!input.is_escalation());
}

#[test]
fn test_complete_review_input_needs_changes() {
    let input = CompleteReviewInput::needs_changes(
        "Missing error handling",
        "Add try-catch blocks around API calls",
    );

    assert_eq!(input.outcome, ReviewToolOutcome::NeedsChanges);
    assert_eq!(input.notes, "Missing error handling");
    assert_eq!(
        input.fix_description,
        Some("Add try-catch blocks around API calls".to_string())
    );
    assert!(input.escalation_reason.is_none());
    assert!(!input.is_approved());
    assert!(input.is_needs_changes());
    assert!(!input.is_escalation());
}

#[test]
fn test_complete_review_input_escalate() {
    let input = CompleteReviewInput::escalate(
        "Security-sensitive authentication changes",
        "Changes bypass the standard auth flow, needs human review",
    );

    assert_eq!(input.outcome, ReviewToolOutcome::Escalate);
    assert_eq!(input.notes, "Security-sensitive authentication changes");
    assert!(input.fix_description.is_none());
    assert_eq!(
        input.escalation_reason,
        Some("Changes bypass the standard auth flow, needs human review".to_string())
    );
    assert!(!input.is_approved());
    assert!(!input.is_needs_changes());
    assert!(input.is_escalation());
}

#[test]
fn test_validate_approved_valid() {
    let input = CompleteReviewInput::approved("All criteria met");
    assert!(input.validate().is_ok());
    assert!(input.is_valid());
}

#[test]
fn test_validate_approved_empty_notes() {
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::Approved,
        notes: "".to_string(),
        issues: Vec::new(),
        fix_description: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert_eq!(
        input.validate(),
        Err(CompleteReviewValidationError::EmptyNotes)
    );
    assert!(!input.is_valid());
}

#[test]
fn test_validate_approved_whitespace_notes() {
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::Approved,
        notes: "   \n\t  ".to_string(),
        issues: Vec::new(),
        fix_description: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert_eq!(
        input.validate(),
        Err(CompleteReviewValidationError::EmptyNotes)
    );
}

#[test]
fn test_validate_needs_changes_valid() {
    let issue = ReviewIssueInput::new("Test issue", IssueSeverity::Major)
        .with_no_step_reason("General issue");
    let input =
        CompleteReviewInput::needs_changes_with_issues("Issues found", "Fix the bug", vec![issue]);
    assert!(input.validate().is_ok());
    assert!(input.is_valid());
}

#[test]
fn test_validate_needs_changes_missing_fix_description() {
    let issue = ReviewIssueInput::new("Test issue", IssueSeverity::Major)
        .with_no_step_reason("General issue");
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::NeedsChanges,
        notes: "Issues found".to_string(),
        issues: vec![issue],
        fix_description: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert_eq!(
        input.validate(),
        Err(CompleteReviewValidationError::MissingFixDescription)
    );
}

#[test]
fn test_validate_needs_changes_empty_fix_description() {
    let issue = ReviewIssueInput::new("Test issue", IssueSeverity::Major)
        .with_no_step_reason("General issue");
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::NeedsChanges,
        notes: "Issues found".to_string(),
        issues: vec![issue],
        fix_description: Some("".to_string()),
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert_eq!(
        input.validate(),
        Err(CompleteReviewValidationError::EmptyFixDescription)
    );
}

#[test]
fn test_validate_needs_changes_whitespace_fix_description() {
    let issue = ReviewIssueInput::new("Test issue", IssueSeverity::Major)
        .with_no_step_reason("General issue");
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::NeedsChanges,
        notes: "Issues found".to_string(),
        issues: vec![issue],
        fix_description: Some("   ".to_string()),
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert_eq!(
        input.validate(),
        Err(CompleteReviewValidationError::EmptyFixDescription)
    );
}

#[test]
fn test_validate_needs_changes_missing_issues() {
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::NeedsChanges,
        notes: "Issues found".to_string(),
        issues: Vec::new(),
        fix_description: Some("Fix bug".to_string()),
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert_eq!(
        input.validate(),
        Err(CompleteReviewValidationError::MissingIssues)
    );
}

#[test]
fn test_validate_needs_changes_invalid_issue() {
    let invalid_issue = ReviewIssueInput::new("Test issue", IssueSeverity::Major);
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::NeedsChanges,
        notes: "Issues found".to_string(),
        issues: vec![invalid_issue],
        fix_description: Some("Fix bug".to_string()),
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert_eq!(
        input.validate(),
        Err(CompleteReviewValidationError::InvalidIssue(
            0,
            ReviewIssueValidationError::MissingStepOrReason
        ))
    );
}

#[test]
fn test_validate_escalate_valid() {
    let input = CompleteReviewInput::escalate("Security concern", "Needs human review");
    assert!(input.validate().is_ok());
    assert!(input.is_valid());
}

#[test]
fn test_validate_escalate_missing_reason() {
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::Escalate,
        notes: "Concern".to_string(),
        issues: Vec::new(),
        fix_description: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert_eq!(
        input.validate(),
        Err(CompleteReviewValidationError::MissingEscalationReason)
    );
}

#[test]
fn test_validate_escalate_empty_reason() {
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::Escalate,
        notes: "Concern".to_string(),
        issues: Vec::new(),
        fix_description: None,
        escalation_reason: Some("".to_string()),
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert_eq!(
        input.validate(),
        Err(CompleteReviewValidationError::EmptyEscalationReason)
    );
}

#[test]
fn test_validate_escalate_whitespace_reason() {
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::Escalate,
        notes: "Concern".to_string(),
        issues: Vec::new(),
        fix_description: None,
        escalation_reason: Some("   ".to_string()),
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert_eq!(
        input.validate(),
        Err(CompleteReviewValidationError::EmptyEscalationReason)
    );
}

#[test]
fn test_review_issue_input_new() {
    let issue = ReviewIssueInput::new("Test issue", IssueSeverity::Critical);
    assert_eq!(issue.title, "Test issue");
    assert_eq!(issue.severity, IssueSeverity::Critical);
    assert!(issue.step_id.is_none());
    assert!(issue.no_step_reason.is_none());
}

#[test]
fn test_review_issue_input_with_step_id() {
    let step_id = TaskStepId("step-123".to_string());
    let issue =
        ReviewIssueInput::new("Test issue", IssueSeverity::Major).with_step_id(step_id.clone());
    assert_eq!(issue.step_id, Some(step_id));
}

#[test]
fn test_review_issue_input_with_no_step_reason() {
    let issue = ReviewIssueInput::new("Test issue", IssueSeverity::Minor)
        .with_no_step_reason("General architecture concern");
    assert_eq!(
        issue.no_step_reason,
        Some("General architecture concern".to_string())
    );
}

#[test]
fn test_review_issue_validate_empty_title() {
    let issue = ReviewIssueInput::new("", IssueSeverity::Major).with_no_step_reason("Reason");
    assert_eq!(
        issue.validate(),
        Err(ReviewIssueValidationError::EmptyTitle)
    );
}

#[test]
fn test_review_issue_validate_missing_step_and_reason() {
    let issue = ReviewIssueInput::new("Test issue", IssueSeverity::Major);
    assert_eq!(
        issue.validate(),
        Err(ReviewIssueValidationError::MissingStepOrReason)
    );
}

#[test]
fn test_review_issue_validate_whitespace_reason() {
    let issue =
        ReviewIssueInput::new("Test issue", IssueSeverity::Major).with_no_step_reason("   ");
    assert_eq!(
        issue.validate(),
        Err(ReviewIssueValidationError::MissingStepOrReason)
    );
}

#[test]
fn test_review_issue_serialization_round_trip() {
    let issue = ReviewIssueInput::new("Bug found", IssueSeverity::Critical)
        .with_no_step_reason("General issue")
        .with_file_location("src/main.rs", Some(42));

    let json = serde_json::to_string(&issue).unwrap();
    let parsed: ReviewIssueInput = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.title, issue.title);
    assert_eq!(parsed.severity, issue.severity);
    assert_eq!(parsed.file_path, issue.file_path);
    assert_eq!(parsed.line_number, issue.line_number);
}

#[test]
fn test_needs_changes_with_multiple_issues() {
    let issue1 =
        ReviewIssueInput::new("Issue 1", IssueSeverity::Major).with_no_step_reason("General");
    let issue2 =
        ReviewIssueInput::new("Issue 2", IssueSeverity::Minor).with_no_step_reason("General");

    let input = CompleteReviewInput::needs_changes_with_issues(
        "Multiple issues",
        "Fix them all",
        vec![issue1, issue2],
    );

    assert_eq!(input.outcome, ReviewToolOutcome::NeedsChanges);
    assert_eq!(input.issues.len(), 2);
    assert!(input.validate().is_ok());
}

#[test]
fn test_review_issue_validation_error_display() {
    assert_eq!(
        ReviewIssueValidationError::EmptyTitle.to_string(),
        "issue title cannot be empty"
    );
    assert_eq!(
        ReviewIssueValidationError::MissingStepOrReason.to_string(),
        "issue must have either step_id or no_step_reason"
    );
}

#[test]
fn test_complete_review_validation_error_display() {
    let err =
        CompleteReviewValidationError::InvalidIssue(2, ReviewIssueValidationError::EmptyTitle);
    assert_eq!(err.to_string(), "issue at index 2: issue title cannot be empty");
}

#[test]
fn test_complete_review_serialization_round_trip_approved() {
    let input = CompleteReviewInput::approved("All good");
    let json = serde_json::to_string(&input).unwrap();
    let parsed: CompleteReviewInput = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.outcome, input.outcome);
    assert_eq!(parsed.notes, input.notes);
}

#[test]
fn test_complete_review_serialization_round_trip_needs_changes() {
    let input = CompleteReviewInput::needs_changes("Issues", "Fix them");
    let json = serde_json::to_string(&input).unwrap();
    let parsed: CompleteReviewInput = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.outcome, input.outcome);
    assert_eq!(parsed.fix_description, input.fix_description);
}

#[test]
fn test_complete_review_serialization_round_trip_escalate() {
    let input = CompleteReviewInput::escalate("Concern", "Need human");
    let json = serde_json::to_string(&input).unwrap();
    let parsed: CompleteReviewInput = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.outcome, input.outcome);
    assert_eq!(parsed.escalation_reason, input.escalation_reason);
}

#[test]
fn test_complete_review_deserialization_missing_optional_fields() {
    let json = r#"{"outcome":"approved","notes":"All good","issues":[]}"#;
    let input: CompleteReviewInput = serde_json::from_str(json).unwrap();
    assert_eq!(input.outcome, ReviewToolOutcome::Approved);
    assert!(input.fix_description.is_none());
    assert!(input.escalation_reason.is_none());
}

#[test]
fn test_complete_review_validation_error_messages() {
    assert_eq!(
        CompleteReviewValidationError::EmptyNotes.to_string(),
        "notes field cannot be empty"
    );
    assert_eq!(
        CompleteReviewValidationError::MissingFixDescription.to_string(),
        "fix_description is required when outcome is 'needs_changes'"
    );
    assert_eq!(
        CompleteReviewValidationError::EmptyFixDescription.to_string(),
        "fix_description cannot be empty when outcome is 'needs_changes'"
    );
    assert_eq!(
        CompleteReviewValidationError::MissingEscalationReason.to_string(),
        "escalation_reason is required when outcome is 'escalate'"
    );
    assert_eq!(
        CompleteReviewValidationError::EmptyEscalationReason.to_string(),
        "escalation_reason cannot be empty when outcome is 'escalate'"
    );
    assert_eq!(
        CompleteReviewValidationError::MissingIssues.to_string(),
        "issues are required when outcome is 'needs_changes'"
    );
}

#[test]
fn test_review_tool_outcome_approved_no_changes_display() {
    assert_eq!(
        format!("{}", ReviewToolOutcome::ApprovedNoChanges),
        "approved_no_changes"
    );
}

#[test]
fn test_review_tool_outcome_approved_no_changes_from_str() {
    assert_eq!(
        ReviewToolOutcome::from_str("approved_no_changes").unwrap(),
        ReviewToolOutcome::ApprovedNoChanges
    );
}

#[test]
fn test_review_tool_outcome_approved_no_changes_from_str_case_insensitive() {
    assert_eq!(
        ReviewToolOutcome::from_str("APPROVED_NO_CHANGES").unwrap(),
        ReviewToolOutcome::ApprovedNoChanges
    );
    assert_eq!(
        ReviewToolOutcome::from_str("Approved_No_Changes").unwrap(),
        ReviewToolOutcome::ApprovedNoChanges
    );
}

#[test]
fn test_validate_approved_no_changes_valid() {
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::ApprovedNoChanges,
        notes: "Research task completed".to_string(),
        issues: Vec::new(),
        fix_description: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert!(input.validate().is_ok());
    assert!(input.is_valid());
    assert!(input.is_approved());
}

#[test]
fn test_validate_approved_no_changes_allows_issues_but_ignored() {
    let issue = ReviewIssueInput::new("some issue", IssueSeverity::Minor)
        .with_no_step_reason("general note");
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::ApprovedNoChanges,
        notes: "Docs task done".to_string(),
        issues: vec![issue],
        fix_description: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert!(input.validate().is_ok());
}

#[test]
fn test_approved_no_changes_is_approved_not_needs_changes_or_escalation() {
    let input = CompleteReviewInput {
        outcome: ReviewToolOutcome::ApprovedNoChanges,
        notes: "No code changes".to_string(),
        issues: Vec::new(),
        fix_description: None,
        escalation_reason: None,
        scope_drift_classification: None,
        scope_drift_notes: None,
    };
    assert!(input.is_approved());
    assert!(!input.is_needs_changes());
    assert!(!input.is_escalation());
}
