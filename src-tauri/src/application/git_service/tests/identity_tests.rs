use super::super::GitService;
use std::fs;
use std::process::Command;

fn git(repo: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn canonical_target_identity_converges_across_linked_worktrees() {
    let repository = tempfile::tempdir().unwrap();
    git(repository.path(), &["init", "-b", "main"]);
    git(
        repository.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(repository.path(), &["config", "user.name", "Test User"]);
    fs::write(repository.path().join("README.md"), "test").unwrap();
    git(repository.path(), &["add", "README.md"]);
    git(repository.path(), &["commit", "-m", "initial"]);
    let worktree_root = tempfile::tempdir().unwrap();
    let worktree = worktree_root.path().join("linked");
    git(
        repository.path(),
        &[
            "worktree",
            "add",
            "-b",
            "feature",
            worktree.to_str().unwrap(),
        ],
    );

    let from_main = GitService::canonical_target_identity(repository.path(), "main")
        .await
        .unwrap();
    let from_linked = GitService::canonical_target_identity(&worktree, "main")
        .await
        .unwrap();

    assert_eq!(from_main, from_linked);
    assert!(from_main.git_common_dir().is_absolute());
    assert_eq!(from_main.full_ref(), "refs/heads/main");
}

#[tokio::test]
async fn canonical_target_identity_rejects_non_local_or_malformed_refs() {
    let repository = tempfile::tempdir().unwrap();
    git(repository.path(), &["init", "-b", "main"]);

    assert!(
        GitService::canonical_target_identity(repository.path(), "refs/tags/v1")
            .await
            .is_err()
    );
    assert!(
        GitService::canonical_target_identity(repository.path(), "bad..branch")
            .await
            .is_err()
    );
}
