use crate::entities::{ReviewOutcome, ScopeDriftStatus};

use super::{ReviewSettings, ReviewToolOutcome, ScopeDriftClassification};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseReviewDecisionError(pub String);

impl std::fmt::Display for ParseReviewDecisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Invalid decision: '{}'. Expected 'approved', 'approved_no_changes', 'needs_changes', or 'escalate'",
            self.0
        )
    }
}

impl std::error::Error for ParseReviewDecisionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompleteReviewPolicyError {
    MissingScopeDriftClassification { out_of_scope_files: Vec<String> },
    CannotApproveUnrelatedDrift,
    EscalationRequiresRevisionExhaustion {
        revision_count: u32,
        max_revision_cycles: u32,
    },
    NeedsChangesRequiresStructuredIssues,
}

impl std::fmt::Display for CompleteReviewPolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompleteReviewPolicyError::MissingScopeDriftClassification { out_of_scope_files } => {
                write!(
                    f,
                    "Scope drift classification required when changed files exceed planned scope: {}",
                    out_of_scope_files.join(", ")
                )
            }
            CompleteReviewPolicyError::CannotApproveUnrelatedDrift => write!(
                f,
                "Cannot approve task with unrelated scope drift; request changes or escalate instead"
            ),
            CompleteReviewPolicyError::EscalationRequiresRevisionExhaustion {
                revision_count,
                max_revision_cycles,
            } => write!(
                f,
                "Unrelated scope drift must go back through revise while revision budget remains ({revision_count}/{max_revision_cycles} used). Use needs_changes with structured issues first, then escalate only if repeated revise cycles fail."
            ),
            CompleteReviewPolicyError::NeedsChangesRequiresStructuredIssues => write!(
                f,
                "Needs-changes for unrelated scope drift requires at least one structured issue so the worker can revise the branch cleanly."
            ),
        }
    }
}

impl std::error::Error for CompleteReviewPolicyError {}

pub fn parse_review_decision(decision: &str) -> Result<ReviewToolOutcome, ParseReviewDecisionError> {
    match decision {
        "approved" => Ok(ReviewToolOutcome::Approved),
        "approved_no_changes" => Ok(ReviewToolOutcome::ApprovedNoChanges),
        "needs_changes" => Ok(ReviewToolOutcome::NeedsChanges),
        "escalate" => Ok(ReviewToolOutcome::Escalate),
        _ => Err(ParseReviewDecisionError(decision.to_string())),
    }
}

pub fn validate_complete_review_policy(
    scope_drift_status: ScopeDriftStatus,
    out_of_scope_files: &[String],
    scope_drift_classification: Option<ScopeDriftClassification>,
    outcome: ReviewToolOutcome,
    revision_count: u32,
    review_settings: &ReviewSettings,
    issue_count: usize,
) -> Result<(), CompleteReviewPolicyError> {
    if matches!(scope_drift_status, ScopeDriftStatus::ScopeExpansion)
        && scope_drift_classification.is_none()
    {
        return Err(CompleteReviewPolicyError::MissingScopeDriftClassification {
            out_of_scope_files: out_of_scope_files.to_vec(),
        });
    }

    if matches!(
        outcome,
        ReviewToolOutcome::Approved | ReviewToolOutcome::ApprovedNoChanges
    ) && matches!(
        scope_drift_classification,
        Some(ScopeDriftClassification::UnrelatedDrift)
    ) {
        return Err(CompleteReviewPolicyError::CannotApproveUnrelatedDrift);
    }

    if matches!(
        scope_drift_classification,
        Some(ScopeDriftClassification::UnrelatedDrift)
    ) {
        if matches!(outcome, ReviewToolOutcome::Escalate)
            && !review_settings.exceeded_max_revisions(revision_count)
        {
            return Err(
                CompleteReviewPolicyError::EscalationRequiresRevisionExhaustion {
                    revision_count,
                    max_revision_cycles: review_settings.max_revision_cycles,
                },
            );
        }

        if matches!(outcome, ReviewToolOutcome::NeedsChanges) && issue_count == 0 {
            return Err(CompleteReviewPolicyError::NeedsChangesRequiresStructuredIssues);
        }
    }

    Ok(())
}

pub fn review_outcome_for_tool(outcome: ReviewToolOutcome) -> ReviewOutcome {
    match outcome {
        ReviewToolOutcome::Approved => ReviewOutcome::Approved,
        ReviewToolOutcome::ApprovedNoChanges => ReviewOutcome::ApprovedNoChanges,
        ReviewToolOutcome::NeedsChanges => ReviewOutcome::ChangesRequested,
        ReviewToolOutcome::Escalate => ReviewOutcome::Rejected,
    }
}

#[cfg(test)]
#[path = "complete_policy_tests.rs"]
mod tests;
