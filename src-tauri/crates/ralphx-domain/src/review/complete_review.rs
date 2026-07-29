use crate::entities::{IssueCategory, IssueSeverity, TaskStepId};
use super::ScopeDriftClassification;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewToolOutcome {
    Approved,
    ApprovedNoChanges,
    NeedsChanges,
    Escalate,
}

impl std::fmt::Display for ReviewToolOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewToolOutcome::Approved => write!(f, "approved"),
            ReviewToolOutcome::ApprovedNoChanges => write!(f, "approved_no_changes"),
            ReviewToolOutcome::NeedsChanges => write!(f, "needs_changes"),
            ReviewToolOutcome::Escalate => write!(f, "escalate"),
        }
    }
}

impl FromStr for ReviewToolOutcome {
    type Err = ParseReviewToolOutcomeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "approved" => Ok(ReviewToolOutcome::Approved),
            "approved_no_changes" => Ok(ReviewToolOutcome::ApprovedNoChanges),
            "needs_changes" => Ok(ReviewToolOutcome::NeedsChanges),
            "escalate" => Ok(ReviewToolOutcome::Escalate),
            _ => Err(ParseReviewToolOutcomeError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseReviewToolOutcomeError(pub String);

impl std::fmt::Display for ParseReviewToolOutcomeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid review tool outcome: '{}', expected 'approved', 'approved_no_changes', 'needs_changes', or 'escalate'",
            self.0
        )
    }
}

impl std::error::Error for ParseReviewToolOutcomeError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewIssueInput {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub severity: IssueSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<IssueCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<TaskStepId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_step_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_number: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_snippet: Option<String>,
}

impl ReviewIssueInput {
    pub fn new(title: impl Into<String>, severity: IssueSeverity) -> Self {
        Self {
            title: title.into(),
            description: None,
            severity,
            category: None,
            step_id: None,
            no_step_reason: None,
            file_path: None,
            line_number: None,
            code_snippet: None,
        }
    }

    pub fn with_step_id(mut self, step_id: TaskStepId) -> Self {
        self.step_id = Some(step_id);
        self
    }

    pub fn with_no_step_reason(mut self, reason: impl Into<String>) -> Self {
        self.no_step_reason = Some(reason.into());
        self
    }

    pub fn with_category(mut self, category: IssueCategory) -> Self {
        self.category = Some(category);
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_file_location(
        mut self,
        file_path: impl Into<String>,
        line_number: Option<i32>,
    ) -> Self {
        self.file_path = Some(file_path.into());
        self.line_number = line_number;
        self
    }

    pub fn validate(&self) -> Result<(), ReviewIssueValidationError> {
        if self.title.trim().is_empty() {
            return Err(ReviewIssueValidationError::EmptyTitle);
        }

        let has_step = self.step_id.is_some();
        let has_reason = self
            .no_step_reason
            .as_ref()
            .is_some_and(|r| !r.trim().is_empty());

        if !has_step && !has_reason {
            return Err(ReviewIssueValidationError::MissingStepOrReason);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewIssueValidationError {
    EmptyTitle,
    MissingStepOrReason,
}

impl std::fmt::Display for ReviewIssueValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewIssueValidationError::EmptyTitle => {
                write!(f, "issue title cannot be empty")
            }
            ReviewIssueValidationError::MissingStepOrReason => {
                write!(f, "issue must have either step_id or no_step_reason")
            }
        }
    }
}

impl std::error::Error for ReviewIssueValidationError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteReviewInput {
    pub outcome: ReviewToolOutcome,
    pub notes: String,
    #[serde(default)]
    pub issues: Vec<ReviewIssueInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalation_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_drift_classification: Option<ScopeDriftClassification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_drift_notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompleteReviewValidationError {
    EmptyNotes,
    MissingFixDescription,
    EmptyFixDescription,
    MissingEscalationReason,
    EmptyEscalationReason,
    MissingIssues,
    InvalidIssue(usize, ReviewIssueValidationError),
}

impl std::fmt::Display for CompleteReviewValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompleteReviewValidationError::EmptyNotes => {
                write!(f, "notes field cannot be empty")
            }
            CompleteReviewValidationError::MissingFixDescription => {
                write!(
                    f,
                    "fix_description is required when outcome is 'needs_changes'"
                )
            }
            CompleteReviewValidationError::EmptyFixDescription => {
                write!(
                    f,
                    "fix_description cannot be empty when outcome is 'needs_changes'"
                )
            }
            CompleteReviewValidationError::MissingEscalationReason => {
                write!(
                    f,
                    "escalation_reason is required when outcome is 'escalate'"
                )
            }
            CompleteReviewValidationError::EmptyEscalationReason => {
                write!(
                    f,
                    "escalation_reason cannot be empty when outcome is 'escalate'"
                )
            }
            CompleteReviewValidationError::MissingIssues => {
                write!(f, "issues are required when outcome is 'needs_changes'")
            }
            CompleteReviewValidationError::InvalidIssue(idx, err) => {
                write!(f, "issue at index {}: {}", idx, err)
            }
        }
    }
}

impl std::error::Error for CompleteReviewValidationError {}

impl CompleteReviewInput {
    pub fn approved(notes: impl Into<String>) -> Self {
        Self {
            outcome: ReviewToolOutcome::Approved,
            notes: notes.into(),
            issues: Vec::new(),
            fix_description: None,
            escalation_reason: None,
            scope_drift_classification: None,
            scope_drift_notes: None,
        }
    }

    pub fn needs_changes(notes: impl Into<String>, fix_description: impl Into<String>) -> Self {
        Self {
            outcome: ReviewToolOutcome::NeedsChanges,
            notes: notes.into(),
            issues: Vec::new(),
            fix_description: Some(fix_description.into()),
            escalation_reason: None,
            scope_drift_classification: None,
            scope_drift_notes: None,
        }
    }

    pub fn needs_changes_with_issues(
        notes: impl Into<String>,
        fix_description: impl Into<String>,
        issues: Vec<ReviewIssueInput>,
    ) -> Self {
        Self {
            outcome: ReviewToolOutcome::NeedsChanges,
            notes: notes.into(),
            issues,
            fix_description: Some(fix_description.into()),
            escalation_reason: None,
            scope_drift_classification: None,
            scope_drift_notes: None,
        }
    }

    pub fn escalate(notes: impl Into<String>, escalation_reason: impl Into<String>) -> Self {
        Self {
            outcome: ReviewToolOutcome::Escalate,
            notes: notes.into(),
            issues: Vec::new(),
            fix_description: None,
            escalation_reason: Some(escalation_reason.into()),
            scope_drift_classification: None,
            scope_drift_notes: None,
        }
    }

    pub fn validate(&self) -> Result<(), CompleteReviewValidationError> {
        if self.notes.trim().is_empty() {
            return Err(CompleteReviewValidationError::EmptyNotes);
        }

        for (idx, issue) in self.issues.iter().enumerate() {
            if let Err(e) = issue.validate() {
                return Err(CompleteReviewValidationError::InvalidIssue(idx, e));
            }
        }

        match self.outcome {
            ReviewToolOutcome::Approved | ReviewToolOutcome::ApprovedNoChanges => Ok(()),
            ReviewToolOutcome::NeedsChanges => {
                match &self.fix_description {
                    None => return Err(CompleteReviewValidationError::MissingFixDescription),
                    Some(desc) if desc.trim().is_empty() => {
                        return Err(CompleteReviewValidationError::EmptyFixDescription);
                    }
                    Some(_) => {}
                }

                if self.issues.is_empty() {
                    return Err(CompleteReviewValidationError::MissingIssues);
                }

                Ok(())
            }
            ReviewToolOutcome::Escalate => match &self.escalation_reason {
                None => Err(CompleteReviewValidationError::MissingEscalationReason),
                Some(reason) if reason.trim().is_empty() => {
                    Err(CompleteReviewValidationError::EmptyEscalationReason)
                }
                Some(_) => Ok(()),
            },
        }
    }

    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }

    pub fn is_approved(&self) -> bool {
        matches!(self.outcome, ReviewToolOutcome::Approved | ReviewToolOutcome::ApprovedNoChanges)
    }

    pub fn is_needs_changes(&self) -> bool {
        self.outcome == ReviewToolOutcome::NeedsChanges
    }

    pub fn is_escalation(&self) -> bool {
        self.outcome == ReviewToolOutcome::Escalate
    }
}

#[cfg(test)]
#[path = "complete_review_tests.rs"]
mod tests;
