use super::super::*;
use super::init_test_repo;
use std::process::Command;

#[tokio::test]
async fn test_restart_move_worktree_rejects_a_locked_source_without_moving_it() {
    let temp_dir = tempfile::tempdir().expect("temporary repository should be created");
    let repo = temp_dir.path();
    init_test_repo(repo);
    let initial_commit = Command::new("git")
        .args(["commit", "--allow-empty", "-m", "initial"])
        .current_dir(repo)
        .output()
        .expect("initial commit should run");
    assert!(
        initial_commit.status.success(),
        "initial commit should succeed"
    );
    let source_branch = Command::new("git")
        .args(["branch", "source-branch"])
        .current_dir(repo)
        .output()
        .expect("source branch creation should run");
    assert!(
        source_branch.status.success(),
        "source branch should be created"
    );

    let source = temp_dir.path().join("source-worktree");
    let destination = temp_dir.path().join("destination-worktree");
    GitService::checkout_existing_branch_worktree(repo, &source, "source-branch")
        .await
        .expect("source worktree should be created");
    let lock_output = Command::new("git")
        .args(["worktree", "lock", source.to_str().unwrap_or_default()])
        .current_dir(repo)
        .output()
        .expect("source worktree lock should run");
    assert!(
        lock_output.status.success(),
        "source worktree should be locked"
    );

    let error = GitService::move_worktree(repo, &source, &destination)
        .await
        .expect_err("moving a locked worktree must fail");
    assert!(error.to_string().contains("Failed to move worktree"));
    assert_eq!(
        GitService::get_current_branch(&source)
            .await
            .expect("source worktree should remain available"),
        "source-branch"
    );
    assert!(
        !destination.exists(),
        "failed move must not create the destination worktree"
    );
}
