// Types for Review commands
// Extracted from review_commands.rs to reduce file size

use serde::{Deserialize, Serialize};

use crate::domain::entities::{Review, ReviewAction, ReviewNote};

// ============================================================================
// Response Types
// ============================================================================

/// Response wrapper for review operations
#[derive(Debug, Serialize)]
pub struct ReviewResponse {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub reviewer_type: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

impl From<Review> for ReviewResponse {
    fn from(review: Review) -> Self {
        Self {
            id: review.id.as_str().to_string(),
            project_id: review.project_id.as_str().to_string(),
            task_id: review.task_id.as_str().to_string(),
            reviewer_type: review.reviewer_type.to_string(),
            status: review.status.to_string(),
            notes: review.notes,
            created_at: review.created_at.to_rfc3339(),
            completed_at: review.completed_at.map(|dt| dt.to_rfc3339()),
        }
    }
}

/// Response wrapper for review actions
#[derive(Debug, Serialize)]
pub struct ReviewActionResponse {
    pub id: String,
    pub review_id: String,
    pub action_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_task_id: Option<String>,
    pub created_at: String,
}

impl From<ReviewAction> for ReviewActionResponse {
    fn from(action: ReviewAction) -> Self {
        Self {
            id: action.id.as_str().to_string(),
            review_id: action.review_id.as_str().to_string(),
            action_type: action.action_type.to_string(),
            target_task_id: action.target_task_id.map(|id| id.as_str().to_string()),
            created_at: action.created_at.to_rfc3339(),
        }
    }
}

/// Issue reported during review
#[derive(Debug, Clone, Serialize)]
pub struct ReviewIssue {
    pub severity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<i32>,
    pub description: String,
}

/// Response wrapper for review notes (state history)
#[derive(Debug, Serialize)]
pub struct ReviewNoteResponse {
    pub id: String,
    pub task_id: String,
    pub reviewer: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issues: Option<Vec<ReviewIssue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followup_session_id: Option<String>,
    pub created_at: String,
}

impl From<ReviewNote> for ReviewNoteResponse {
    fn from(note: ReviewNote) -> Self {
        // Convert domain issues to command issues
        let issues = note.issues.map(|issues| {
            issues
                .into_iter()
                .map(|i| ReviewIssue {
                    severity: i.severity,
                    file: i.file,
                    line: i.line,
                    description: i.description,
                })
                .collect()
        });

        Self {
            id: note.id.as_str().to_string(),
            task_id: note.task_id.as_str().to_string(),
            reviewer: note.reviewer.to_string(),
            outcome: note.outcome.to_string(),
            summary: note.summary,
            notes: note.notes,
            issues,
            followup_session_id: note.followup_session_id,
            created_at: note.created_at.to_rfc3339(),
        }
    }
}

// ============================================================================
// Input Types
// ============================================================================

/// Input for approving a review
#[derive(Debug, Deserialize)]
pub struct ApproveReviewInput {
    pub review_id: String,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Input for requesting changes on a review
#[derive(Debug, Deserialize)]
pub struct RequestChangesInput {
    pub review_id: String,
    pub notes: String,
    #[serde(default)]
    pub fix_description: Option<String>,
}

/// Input for rejecting a review
#[derive(Debug, Deserialize)]
pub struct RejectReviewInput {
    pub review_id: String,
    pub notes: String,
}

/// Input for approving a fix task
#[derive(Debug, Deserialize)]
pub struct ApproveFixTaskInput {
    pub fix_task_id: String,
}

/// Input for rejecting a fix task
#[derive(Debug, Deserialize)]
pub struct RejectFixTaskInput {
    pub fix_task_id: String,
    pub feedback: String,
    pub original_task_id: String,
}

/// Response for fix task attempt count
#[derive(Debug, Serialize)]
pub struct FixTaskAttemptsResponse {
    pub task_id: String,
    pub attempt_count: u32,
}

// ============================================================================
// Task-based Review Input Types
// ============================================================================

/// Input for approving a task after AI review has passed or escalated
#[derive(Debug, Deserialize)]
pub struct ApproveTaskInput {
    pub task_id: String,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Input for requesting changes on a task after AI review has passed or escalated
#[derive(Debug, Deserialize)]
pub struct RequestTaskChangesInput {
    pub task_id: String,
    pub feedback: String,
}

/// Input for requesting changes on a task while it is actively being reviewed (Reviewing state)
/// Stops the reviewer agent, records a human ChangesRequested note, and transitions to RevisionNeeded.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestTaskChangesFromReviewingInput {
    pub task_id: String,
    pub feedback: String,
}

/// Input for re-queueing an escalated task for AI re-review
#[derive(Debug, Deserialize)]
pub struct ReReviewTaskInput {
    pub task_id: String,
}

// ============================================================================
// Review Issue Types
// ============================================================================

use crate::domain::entities::{
    IssueProgressSummary, ReviewIssueEntity as ReviewIssueDomain, SeverityBreakdown, SeverityCount,
};

/// Response wrapper for review issues
#[derive(Debug, Serialize)]
pub struct ReviewIssueResponse {
    pub id: String,
    pub review_note_id: String,
    pub task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_step_reason: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub severity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_number: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_snippet: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addressed_in_attempt: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_by_review_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<ReviewIssueDomain> for ReviewIssueResponse {
    fn from(issue: ReviewIssueDomain) -> Self {
        Self {
            id: issue.id.as_str().to_string(),
            review_note_id: issue.review_note_id.as_str().to_string(),
            task_id: issue.task_id.as_str().to_string(),
            step_id: issue.step_id.map(|id| id.as_str().to_string()),
            no_step_reason: issue.no_step_reason,
            title: issue.title,
            description: issue.description,
            severity: issue.severity.to_db_string().to_string(),
            category: issue.category.map(|c| c.to_db_string().to_string()),
            file_path: issue.file_path,
            line_number: issue.line_number,
            code_snippet: issue.code_snippet,
            status: issue.status.to_db_string().to_string(),
            resolution_notes: issue.resolution_notes,
            addressed_in_attempt: issue.addressed_in_attempt,
            verified_by_review_id: issue
                .verified_by_review_id
                .map(|id| id.as_str().to_string()),
            created_at: issue.created_at.to_rfc3339(),
            updated_at: issue.updated_at.to_rfc3339(),
        }
    }
}

/// Response wrapper for issue progress summary
#[derive(Debug, Serialize)]
pub struct IssueProgressResponse {
    pub task_id: String,
    pub total: u32,
    pub open: u32,
    pub in_progress: u32,
    pub addressed: u32,
    pub verified: u32,
    pub wontfix: u32,
    pub percent_resolved: f32,
    pub by_severity: SeverityBreakdownResponse,
}

/// Response wrapper for severity breakdown
#[derive(Debug, Serialize)]
pub struct SeverityBreakdownResponse {
    pub critical: SeverityCountResponse,
    pub major: SeverityCountResponse,
    pub minor: SeverityCountResponse,
    pub suggestion: SeverityCountResponse,
}

/// Response wrapper for severity count
#[derive(Debug, Serialize)]
pub struct SeverityCountResponse {
    pub total: u32,
    pub open: u32,
    pub resolved: u32,
}

impl From<SeverityCount> for SeverityCountResponse {
    fn from(count: SeverityCount) -> Self {
        Self {
            total: count.total,
            open: count.open,
            resolved: count.resolved,
        }
    }
}

impl From<SeverityBreakdown> for SeverityBreakdownResponse {
    fn from(breakdown: SeverityBreakdown) -> Self {
        Self {
            critical: SeverityCountResponse::from(breakdown.critical),
            major: SeverityCountResponse::from(breakdown.major),
            minor: SeverityCountResponse::from(breakdown.minor),
            suggestion: SeverityCountResponse::from(breakdown.suggestion),
        }
    }
}

impl From<IssueProgressSummary> for IssueProgressResponse {
    fn from(summary: IssueProgressSummary) -> Self {
        Self {
            task_id: summary.task_id.as_str().to_string(),
            total: summary.total,
            open: summary.open,
            in_progress: summary.in_progress,
            addressed: summary.addressed,
            verified: summary.verified,
            wontfix: summary.wontfix,
            percent_resolved: summary.percent_resolved,
            by_severity: SeverityBreakdownResponse::from(summary.by_severity),
        }
    }
}

/// Input for verifying an issue
#[derive(Debug, Deserialize)]
pub struct VerifyIssueInput {
    pub issue_id: String,
    pub review_note_id: String,
}

/// Input for reopening an issue
#[derive(Debug, Deserialize)]
pub struct ReopenIssueInput {
    pub issue_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Input for marking an issue as in progress
#[derive(Debug, Deserialize)]
pub struct MarkIssueInProgressInput {
    pub issue_id: String,
}

/// Input for marking an issue as addressed
#[derive(Debug, Deserialize)]
pub struct MarkIssueAddressedInput {
    pub issue_id: String,
    pub resolution_notes: String,
    pub attempt_number: i32,
}

// ============================================================================
// Review Settings Types
// ============================================================================

/// Response for global review settings
#[derive(Debug, Serialize)]
pub struct ReviewSettingsResponse {
    pub require_human_review: bool,
    pub require_workspace_review: bool,
    pub max_fix_attempts: u32,
    pub max_revision_cycles: u32,
    /// Stored-only; follow-up decision pending
    pub ai_review_enabled: bool,
    /// Stored-only; follow-up decision pending
    pub ai_review_auto_fix: bool,
    /// Stored-only; follow-up decision pending
    pub require_fix_approval: bool,
    /// Whether eligible registered agent issues auto-create visible follow-up Agent conversations.
    pub auto_create_followup_agent_conversation: bool,
    /// Whether blocking Workspace Review findings automatically spawn the workspace repair agent.
    pub autofix_workspace_review_blocking_findings: bool,
    /// Maximum automatic workspace Review fixer cycles; zero disables automatic routing.
    pub workspace_review_fixer_cycle_cap: i64,
    /// Whether task execution agents may run backend-managed validation commands.
    pub run_task_validations: bool,
}

/// Input for updating the primary review policy fields.
/// Ballast fields (ai_review_enabled, ai_review_auto_fix, require_fix_approval)
/// are preserved as-is; only provide fields you want to change.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReviewSettingsInput {
    pub require_human_review: Option<bool>,
    pub require_workspace_review: Option<bool>,
    pub max_fix_attempts: Option<u32>,
    pub max_revision_cycles: Option<u32>,
    pub auto_create_followup_agent_conversation: Option<bool>,
    pub autofix_workspace_review_blocking_findings: Option<bool>,
    pub workspace_review_fixer_cycle_cap: Option<i64>,
    pub run_task_validations: Option<bool>,
}

use crate::domain::review::ReviewSettings;

impl From<ReviewSettings> for ReviewSettingsResponse {
    fn from(s: ReviewSettings) -> Self {
        Self {
            require_human_review: s.require_human_review,
            require_workspace_review: s.require_workspace_review,
            max_fix_attempts: s.max_fix_attempts,
            max_revision_cycles: s.max_revision_cycles,
            ai_review_enabled: s.ai_review_enabled,
            ai_review_auto_fix: s.ai_review_auto_fix,
            require_fix_approval: s.require_fix_approval,
            auto_create_followup_agent_conversation: s.auto_create_followup_agent_conversation,
            autofix_workspace_review_blocking_findings: s
                .autofix_workspace_review_blocking_findings,
            workspace_review_fixer_cycle_cap: s.workspace_review_fixer_cycle_cap,
            run_task_validations: s.run_task_validations,
        }
    }
}
