// Tests for update_source_from_target — ensures feature/task branches are brought
// up-to-date from their target branch before the merge runs, preventing false
// validation failures from stale code.

use super::super::merge_coordination::{update_source_from_target, SourceUpdateResult};
use super::helpers::*;
use crate::domain::entities::Project;

/// Helper: create a project entity pointing at a repo path.
fn make_test_project(repo_path: &str) -> Project {
    make_real_git_project(repo_path)
}

// ==================
// AlreadyUpToDate
// ==================

#[tokio::test]
async fn source_up_to_date_with_target_returns_already_up_to_date() {
    // Setup: task branch created from main, no new commits on main since
    let repo = setup_real_git_repo();
    let project = make_test_project(&repo.path_string());

    let result = update_source_from_target(
        repo.path(),
        &repo.task_branch, // source = task branch (has main's commit as ancestor)
        "main",            // target = main
        &project,
        "task-1",
        None,
    )
    .await;

    assert!(
        matches!(result, SourceUpdateResult::AlreadyUpToDate),
        "Source branch created from main (no divergence) should be AlreadyUpToDate. Got: {:?}",
        result
    );
}

// ==================
// Updated
// ==================

#[tokio::test]
async fn source_behind_target_gets_updated() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Add a fix commit to main AFTER the task branch was created
    std::fs::write(path.join("hotfix.rs"), "// hotfix on main").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "hotfix.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: hotfix on main"])
        .current_dir(path)
        .output();

    let project = make_test_project(&repo.path_string());

    let result = update_source_from_target(
        path,
        &repo.task_branch, // source = task branch (behind main now)
        "main",            // target = main (has new commit)
        &project,
        "task-2",
        None,
    )
    .await;

    assert!(
        matches!(result, SourceUpdateResult::Updated),
        "Source branch behind target should be Updated. Got: {:?}",
        result
    );

    // Verify: main's hotfix commit should now be on the task branch
    let log_output = std::process::Command::new("git")
        .args(["log", "--oneline", &repo.task_branch])
        .current_dir(path)
        .output()
        .expect("git log");
    let log_str = String::from_utf8_lossy(&log_output.stdout);
    assert!(
        log_str.contains("hotfix"),
        "Task branch should now contain the hotfix commit from main. Log:\n{}",
        log_str,
    );
}

#[tokio::test]
async fn source_behind_plan_branch_gets_updated() {
    // Test: task branch behind its plan branch target (not main)
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Create plan branch from main
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", "plan/feature-1"])
        .current_dir(path)
        .output();

    // Add a commit on plan branch (simulating another task merged into it)
    std::fs::write(path.join("other_task.rs"), "// other task's work").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "other_task.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "feat: other task merged into plan"])
        .current_dir(path)
        .output();

    // Switch back to main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();

    let project = make_test_project(&repo.path_string());

    // task_branch was created from main (before plan branch got the extra commit)
    // so it's behind plan/feature-1
    let result = update_source_from_target(
        path,
        &repo.task_branch, // source = task branch
        "plan/feature-1",  // target = plan branch (has extra commit)
        &project,
        "task-3",
        None,
    )
    .await;

    assert!(
        matches!(result, SourceUpdateResult::Updated),
        "Task branch behind plan branch should be Updated. Got: {:?}",
        result
    );

    // Verify: plan branch's commit should now be on the task branch
    let log_output = std::process::Command::new("git")
        .args(["log", "--oneline", &repo.task_branch])
        .current_dir(path)
        .output()
        .expect("git log");
    let log_str = String::from_utf8_lossy(&log_output.stdout);
    assert!(
        log_str.contains("other task"),
        "Task branch should now contain the plan branch commit. Log:\n{}",
        log_str,
    );
}

// ==================
// Conflicts
// ==================

#[tokio::test]
async fn source_behind_target_with_conflicts_returns_conflicts() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Add a conflicting change on main (same file the task branch modifies)
    std::fs::write(
        path.join("feature.rs"),
        "// main conflicting version\nfn main_feature() {}",
    )
    .unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "feature.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args([
            "commit",
            "-m",
            "fix: conflicting change to feature.rs on main",
        ])
        .current_dir(path)
        .output();

    let project = make_test_project(&repo.path_string());

    let result = update_source_from_target(
        path,
        &repo.task_branch, // source = task branch (has feature.rs with different content)
        "main",            // target = main (also modified feature.rs)
        &project,
        "task-4",
        None,
    )
    .await;

    assert!(
        matches!(result, SourceUpdateResult::Conflicts { .. }),
        "Conflicting changes should return Conflicts. Got: {:?}",
        result
    );
}

// ==================
// Error: nonexistent branches
// ==================

#[tokio::test]
async fn missing_repo_path_returns_error_before_branch_checks() {
    let temp = tempfile::tempdir().expect("temp dir");
    let missing_repo_path = temp.path().join("missing-repo");
    let missing_repo_path_str = missing_repo_path.to_string_lossy().to_string();
    let project = make_test_project(&missing_repo_path_str);

    let result = update_source_from_target(
        &missing_repo_path,
        "task/source",
        "main",
        &project,
        "task-missing-repo",
        None,
    )
    .await;

    match result {
        SourceUpdateResult::Error(message) => {
            assert!(message.contains("repository path does not exist"));
            assert!(message.contains("missing-repo"));
        }
        other => panic!("missing repository should return Error, got {other:?}"),
    }
}

#[tokio::test]
async fn nonexistent_target_branch_returns_branch_missing() {
    let repo = setup_real_git_repo();
    let project = make_test_project(&repo.path_string());

    let result = update_source_from_target(
        repo.path(),
        &repo.task_branch,
        "nonexistent-target", // target doesn't exist
        &project,
        "task-5",
        None,
    )
    .await;

    assert!(
        matches!(result, SourceUpdateResult::BranchMissing { .. }),
        "Nonexistent target branch should return BranchMissing. Got: {:?}",
        result
    );
}

#[tokio::test]
async fn nonexistent_source_branch_returns_branch_missing() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Add a commit to main so there's divergence to trigger the update path
    std::fs::write(path.join("extra.rs"), "// extra").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "extra.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "chore: extra commit"])
        .current_dir(path)
        .output();

    let project = make_test_project(&repo.path_string());

    let result = update_source_from_target(
        path,
        "nonexistent-source", // source doesn't exist
        "main",
        &project,
        "task-6",
        None,
    )
    .await;

    assert!(
        matches!(result, SourceUpdateResult::BranchMissing { .. }),
        "Nonexistent source branch should return BranchMissing. Got: {:?}",
        result
    );
}

// ==================
// Idempotency
// ==================

#[tokio::test]
async fn update_is_idempotent_second_call_returns_already_up_to_date() {
    let repo = setup_real_git_repo();
    let path = repo.path();

    // Add a commit to main to make task branch behind
    std::fs::write(path.join("hotfix2.rs"), "// hotfix 2").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "hotfix2.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: hotfix 2"])
        .current_dir(path)
        .output();

    let project = make_test_project(&repo.path_string());

    // First call: should update
    let result1 =
        update_source_from_target(path, &repo.task_branch, "main", &project, "task-7", None).await;
    assert!(
        matches!(result1, SourceUpdateResult::Updated),
        "First call should update. Got: {:?}",
        result1
    );

    // Second call: should be up-to-date
    let result2 =
        update_source_from_target(path, &repo.task_branch, "main", &project, "task-7", None).await;
    assert!(
        matches!(result2, SourceUpdateResult::AlreadyUpToDate),
        "Second call should be AlreadyUpToDate. Got: {:?}",
        result2
    );
}
