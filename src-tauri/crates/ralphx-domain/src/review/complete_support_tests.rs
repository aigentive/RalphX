use super::*;
use crate::entities::{ProjectId, ScopeDriftStatus, Task, TaskContext, TaskProposalSummary};

#[test]
fn test_parse_review_issue_backfills_title_and_reason() {
    let issue = RawReviewIssueInput {
        severity: "major".to_string(),
        title: None,
        step_id: None,
        no_step_reason: None,
        description: Some("Missing tests".to_string()),
        category: Some("missing".to_string()),
        file_path: Some("src/lib.rs".to_string()),
        line_number: Some(12),
        code_snippet: None,
    };

    let parsed = parse_review_issue(&issue).unwrap();
    assert_eq!(parsed.title, "Missing tests");
    assert_eq!(
        parsed.no_step_reason.as_deref(),
        Some("Reviewer did not associate this issue with a specific task step")
    );
    assert_eq!(parsed.line_number, Some(12));
}

#[test]
fn test_parse_review_issue_rejects_unknown_severity() {
    let issue = RawReviewIssueInput {
        severity: "weird".to_string(),
        title: Some("Bad".to_string()),
        step_id: None,
        no_step_reason: Some("General".to_string()),
        description: None,
        category: None,
        file_path: None,
        line_number: None,
        code_snippet: None,
    };

    let err = parse_review_issue(&issue).unwrap_err();
    assert!(err.contains("Invalid issue severity"));
}

#[test]
fn test_build_unrelated_drift_followup_prompt_includes_scope_context() {
    let task = Task::new(ProjectId::new(), "Fix scope drift".to_string());
    let context = TaskContext {
        task: task.clone().into(),
        source_proposal: Some(TaskProposalSummary {
            affected_paths: vec!["src-tauri/src/http_server".to_string()],
            ..Default::default()
        }),
        plan_artifact: None,
        blueprint_artifact: None,
        related_artifacts: Vec::new(),
        steps: Vec::new(),
        step_progress: None,
        context_hints: Vec::new(),
        blocked_by: Vec::new(),
        blocks: Vec::new(),
        tier: None,
        task_branch: None,
        worktree_path: None,
        validation_cache: None,
        actual_changed_files: vec!["ralphx.yaml".to_string()],
        scope_drift_status: ScopeDriftStatus::ScopeExpansion,
        out_of_scope_files: vec!["ralphx.yaml".to_string()],
        out_of_scope_blocker_fingerprint: None,
        followup_sessions: Vec::new(),
    };

    let prompt = build_unrelated_drift_followup_prompt(
        &task,
        &context,
        Some("Summary"),
        Some("Feedback"),
        Some("Reason"),
        2,
        3,
    );

    assert!(prompt.contains("Fix scope drift"));
    assert!(prompt.contains("src-tauri/src/http_server"));
    assert!(prompt.contains("ralphx.yaml"));
    assert!(prompt.contains("2/3 revise cycles"));
}
