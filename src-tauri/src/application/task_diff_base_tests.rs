use crate::application::task_diff_base::{
    captured_task_diff_base, task_allows_empty_captured_diff,
};
use crate::domain::entities::{ProjectId, Task, TaskCategory};

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
