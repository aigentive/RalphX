use crate::application::task_diff_base::{
    captured_task_diff_base, ensure_task_has_non_empty_captured_diff,
    task_allows_empty_captured_diff, EMPTY_TASK_DIFF_MISSING_CAPTURED_BASE_REASON,
};
use crate::domain::entities::{ProjectId, Task, TaskCategory};
use crate::error::AppError;

#[test]
fn captured_task_diff_base_uses_sha_as_effective_ref_and_branch_as_display() {
    let mut task = Task::new(ProjectId::new(), "captured base".to_string());
    task.task_branch_base_ref = Some("ralphx/project/agent-plan".to_string());
    task.task_branch_base_sha = Some("abc123base".to_string());

    let base = captured_task_diff_base(&task).expect("captured base");

    assert_eq!(base.effective_base_ref, "abc123base");
    assert_eq!(base.display_base_ref, "ralphx/project/agent-plan");
    assert!(base.immutable);
}

#[test]
fn captured_task_diff_base_uses_sha_as_display_when_ref_missing() {
    let mut task = Task::new(ProjectId::new(), "captured base".to_string());
    task.task_branch_base_sha = Some("abc123base".to_string());

    let base = captured_task_diff_base(&task).expect("captured base");

    assert_eq!(base.effective_base_ref, "abc123base");
    assert_eq!(base.display_base_ref, "abc123base");
    assert!(base.immutable);
}

#[test]
fn task_allows_empty_captured_diff_for_explicit_no_code_or_plan_merge() {
    let mut task = Task::new(ProjectId::new(), "no code".to_string());
    assert!(!task_allows_empty_captured_diff(&task));

    task.metadata = Some(r#"{"no_code_changes":true}"#.to_string());
    assert!(task_allows_empty_captured_diff(&task));

    let mut plan_merge = Task::new_with_category(
        ProjectId::new(),
        "plan merge".to_string(),
        TaskCategory::PlanMerge,
    );
    plan_merge.metadata = None;
    assert!(task_allows_empty_captured_diff(&plan_merge));
}

#[tokio::test]
async fn ensure_non_empty_captured_diff_blocks_code_change_without_captured_base() {
    let task = Task::new(ProjectId::new(), "legacy code-change task".to_string());

    let error = ensure_task_has_non_empty_captured_diff(&task, "unit_test")
        .await
        .expect_err("legacy code-change task without captured base must fail closed");

    assert!(
        matches!(error, AppError::ExecutionBlocked(ref message)
            if message.contains(EMPTY_TASK_DIFF_MISSING_CAPTURED_BASE_REASON)
                && message.contains(task.id.as_str())
                && message.contains("unit_test")),
        "expected structured missing captured-base block, got {error:?}"
    );
}

#[tokio::test]
async fn ensure_non_empty_captured_diff_allows_explicit_no_code_without_captured_base() {
    let mut task = Task::new(ProjectId::new(), "explicit no-code task".to_string());
    task.metadata = Some(r#"{"no_code_changes":true}"#.to_string());

    ensure_task_has_non_empty_captured_diff(&task, "unit_test")
        .await
        .expect("explicit no-code tasks may complete without captured diff metadata");
}
