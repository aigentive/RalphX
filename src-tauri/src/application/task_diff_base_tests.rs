use crate::application::task_diff_base::{
    captured_task_diff_base, ensure_task_has_non_empty_captured_diff,
    task_allows_empty_captured_diff, EMPTY_TASK_DIFF_MISSING_CAPTURED_BASE_REASON,
};
use crate::domain::entities::{Project, ProjectId, Task, TaskCategory};
use crate::error::AppError;
use std::path::Path;
use std::process::Command;

fn test_project_at(path: &Path) -> Project {
    Project::new(
        "task diff base test".to_string(),
        path.to_string_lossy().into_owned(),
    )
}

fn run_git(repo_path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_repo_with_captured_base() -> (tempfile::TempDir, String) {
    let cwd = std::env::current_dir().expect("current dir");
    let temp = tempfile::Builder::new()
        .prefix("task-diff-base-")
        .tempdir_in(cwd)
        .expect("tempdir");
    let repo_path = temp.path();

    run_git(repo_path, &["init", "-b", "main"]);
    run_git(repo_path, &["config", "user.email", "test@example.com"]);
    run_git(repo_path, &["config", "user.name", "Test User"]);
    std::fs::write(repo_path.join("README.md"), "base\n").expect("write base file");
    run_git(repo_path, &["add", "README.md"]);
    run_git(repo_path, &["commit", "-m", "base"]);
    let base_sha = run_git(repo_path, &["rev-parse", "HEAD"]);

    (temp, base_sha)
}

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
    let project = test_project_at(Path::new("/tmp/unused-for-missing-captured-base"));

    let error = ensure_task_has_non_empty_captured_diff(&task, &project, "unit_test")
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
    let project = test_project_at(Path::new("/tmp/unused-for-no-code-task"));

    ensure_task_has_non_empty_captured_diff(&task, &project, "unit_test")
        .await
        .expect("explicit no-code tasks may complete without captured diff metadata");
}

#[tokio::test]
async fn ensure_non_empty_captured_diff_uses_project_checkout_when_worktree_path_missing() {
    let (temp, base_sha) = setup_repo_with_captured_base();
    let repo_path = temp.path();
    run_git(repo_path, &["checkout", "-b", "task/non-worktree"]);
    std::fs::write(repo_path.join("README.md"), "base\nchange\n").expect("write task change");
    run_git(repo_path, &["add", "README.md"]);
    run_git(repo_path, &["commit", "-m", "task change"]);

    let project = test_project_at(repo_path);
    let mut task = Task::new(project.id.clone(), "non-worktree task".to_string());
    task.task_branch = Some("task/non-worktree".to_string());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(base_sha);
    task.worktree_path = None;

    ensure_task_has_non_empty_captured_diff(&task, &project, "unit_test")
        .await
        .expect("captured diff guard should use project checkout when no worktree is persisted");
}

#[tokio::test]
async fn ensure_non_empty_captured_diff_blocks_project_checkout_on_wrong_task_branch() {
    let (temp, base_sha) = setup_repo_with_captured_base();
    let repo_path = temp.path();
    std::fs::write(repo_path.join("README.md"), "base\nsibling\n").expect("write sibling change");
    run_git(repo_path, &["add", "README.md"]);
    run_git(repo_path, &["commit", "-m", "sibling change"]);

    let project = test_project_at(repo_path);
    let mut task = Task::new(project.id.clone(), "missing worktree task".to_string());
    task.task_branch = Some("task/missing-worktree".to_string());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(base_sha);
    task.worktree_path = None;

    let error = ensure_task_has_non_empty_captured_diff(&task, &project, "unit_test")
        .await
        .expect_err("wrong project checkout branch must not count as task-owned diff");

    assert!(
        matches!(error, AppError::ExecutionBlocked(ref message)
            if message.contains("project checkout is on branch")
                && message.contains("task/missing-worktree")
                && message.contains("unit_test")),
        "expected branch mismatch block, got {error:?}"
    );
}
