use crate::entities::{IssueCategory, IssueSeverity, Task, TaskContext, TaskStepId};

const DEFAULT_REVIEW_ISSUE_TITLE: &str = "Review issue";
const DEFAULT_NO_STEP_REASON: &str =
    "Reviewer did not associate this issue with a specific task step";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawReviewIssueInput {
    pub severity: String,
    pub title: Option<String>,
    pub step_id: Option<String>,
    pub no_step_reason: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub file_path: Option<String>,
    pub line_number: Option<u32>,
    pub code_snippet: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedReviewIssue {
    pub title: String,
    pub description: Option<String>,
    pub severity: IssueSeverity,
    pub category: Option<IssueCategory>,
    pub step_id: Option<TaskStepId>,
    pub no_step_reason: Option<String>,
    pub file_path: Option<String>,
    pub line_number: Option<i32>,
    pub code_snippet: Option<String>,
}

pub fn parse_review_issues(issues: &[RawReviewIssueInput]) -> Result<Vec<ParsedReviewIssue>, String> {
    issues.iter().map(parse_review_issue).collect()
}

pub fn parse_review_issue(issue: &RawReviewIssueInput) -> Result<ParsedReviewIssue, String> {
    let severity = IssueSeverity::from_db_string(&issue.severity).map_err(|_| {
        format!(
            "Invalid issue severity: '{}'. Expected 'critical', 'major', 'minor', or 'suggestion'",
            issue.severity
        )
    })?;
    let category = issue
        .category
        .as_deref()
        .map(IssueCategory::from_db_string)
        .transpose()
        .map_err(|_| {
            format!(
                "Invalid issue category: '{}'. Expected 'bug', 'missing', 'quality', or 'design'",
                issue.category.as_deref().unwrap_or_default()
            )
        })?;
    let step_id = issue.step_id.as_deref().map(TaskStepId::from_string);
    let title = issue
        .title
        .clone()
        .or_else(|| issue.description.clone())
        .unwrap_or_else(|| DEFAULT_REVIEW_ISSUE_TITLE.to_string());
    let no_step_reason = match (&step_id, &issue.no_step_reason) {
        (Some(_), _) => None,
        (None, Some(reason)) if !reason.trim().is_empty() => Some(reason.clone()),
        (None, _) => Some(DEFAULT_NO_STEP_REASON.to_string()),
    };

    Ok(ParsedReviewIssue {
        title,
        description: issue.description.clone(),
        severity,
        category,
        step_id,
        no_step_reason,
        file_path: issue.file_path.clone(),
        line_number: issue.line_number.map(|line| line as i32),
        code_snippet: issue.code_snippet.clone(),
    })
}

pub fn build_unrelated_drift_followup_prompt(
    task: &Task,
    task_context: &TaskContext,
    summary: Option<&str>,
    feedback: Option<&str>,
    escalation_reason: Option<&str>,
    revision_count: u32,
    max_revision_cycles: u32,
) -> String {
    let planned_paths = task_context
        .source_proposal
        .as_ref()
        .map(|proposal| proposal.affected_paths.clone())
        .unwrap_or_default();

    format!(
        "This ideation follow-up was spawned automatically from AI review because task '{title}' \
could not be kept within scope after {revision_count}/{max_revision_cycles} revise cycles.\n\n\
Source task id: {task_id}\n\
Reason: unrelated out-of-scope drift blocked clean approval and merge.\n\
Review summary: {summary}\n\
Review feedback: {feedback}\n\
Escalation reason: {escalation_reason}\n\
Planned scope: {planned_scope}\n\
Out-of-scope files: {out_of_scope}\n\
Actual changed files: {actual_changed}\n\n\
Your job is to create isolated follow-up work that addresses the blocker separately from the \
original accepted session. Do not mutate the accepted parent session; instead propose standalone \
follow-up tasks for the unrelated work needed to resolve this blocker cleanly.",
        title = task.title,
        revision_count = revision_count,
        max_revision_cycles = max_revision_cycles,
        task_id = task.id.as_str(),
        summary = summary.unwrap_or("(none)"),
        feedback = feedback.unwrap_or("(none)"),
        escalation_reason = escalation_reason.unwrap_or("(none)"),
        planned_scope = if planned_paths.is_empty() {
            "(none recorded)".to_string()
        } else {
            planned_paths.join(", ")
        },
        out_of_scope = if task_context.out_of_scope_files.is_empty() {
            "(none)".to_string()
        } else {
            task_context.out_of_scope_files.join(", ")
        },
        actual_changed = if task_context.actual_changed_files.is_empty() {
            "(none)".to_string()
        } else {
            task_context.actual_changed_files.join(", ")
        },
    )
}

#[cfg(test)]
#[path = "complete_support_tests.rs"]
mod tests;
