use super::*;
use std::process::Command;
use tempfile::TempDir;

/// Set up a git repo with an initial commit on `main`.
fn setup_git_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(path)
        .output()
        .unwrap();
    dir
}

/// enable_feature_branch auto-creation path: non-existent base branch is created from
/// project default, returning `was_created = true`.
#[tokio::test]
async fn test_ensure_base_branch_auto_creation_for_enable_feature_branch() {
    let dir = setup_git_repo();
    let non_existent = "feature/auto-created";

    // Mirrors the call inside enable_feature_branch after the replacement:
    // ensure_base_branch_exists(&repo_path, &base_branch, project.base_branch.as_deref()).await?
    let was_created = ensure_base_branch_exists(dir.path(), non_existent, Some("main"))
        .await
        .expect("ensure_base_branch_exists should succeed");

    assert!(was_created, "Expected branch to be created (was_created = true)");

    // Verify the branch actually exists in git
    let output = Command::new("git")
        .args(["branch", "--list", non_existent])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(non_existent),
        "Expected branch '{}' to exist after auto-creation",
        non_existent
    );
}

/// Existing branch: ensure_base_branch_exists returns false (no-op, no re-creation).
#[tokio::test]
async fn test_ensure_base_branch_existing_branch_no_op() {
    let dir = setup_git_repo();

    let was_created = ensure_base_branch_exists(dir.path(), "main", None)
        .await
        .expect("ensure_base_branch_exists should succeed for existing branch");

    assert!(!was_created, "Expected was_created = false for existing branch");
}
