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

/// branch exists → returns Ok(false), no-op
#[tokio::test]
async fn test_branch_exists_returns_false() {
    let dir = setup_git_repo();
    let result = ensure_base_branch_exists(dir.path(), "main", None).await;
    assert_eq!(result, Ok(false));
}

/// branch doesn't exist → creates it, returns Ok(true)
#[tokio::test]
async fn test_branch_not_exists_creates_and_returns_true() {
    let dir = setup_git_repo();
    let result = ensure_base_branch_exists(dir.path(), "release/v2", Some("main")).await;
    assert_eq!(result, Ok(true));
    // Verify the branch was actually created
    let output = Command::new("git")
        .args(["branch", "--list", "release/v2"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("release/v2"), "Expected branch to exist after creation");
}

/// creation fails (invalid source branch) → error propagated as String
#[tokio::test]
async fn test_creation_failure_propagated_as_string() {
    let dir = setup_git_repo();
    let result =
        ensure_base_branch_exists(dir.path(), "new-branch", Some("nonexistent-source")).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("Failed to create branch"),
        "Expected error message to contain 'Failed to create branch', got: {}",
        err
    );
}
