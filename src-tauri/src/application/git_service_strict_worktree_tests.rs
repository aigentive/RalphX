use std::path::Path;
use std::process::Command;

use crate::application::GitService;
use crate::error::AppError;
use crate::utils::path_safety::validate_absolute_non_root_path;

fn git(repo: &Path, args: &[&str]) {
    let repo = validate_absolute_non_root_path(repo, "strict worktree test repository")
        .expect("test repository path should be safe");
    let output = Command::new("git")
        .args(args)
        // codeql[rust/path-injection]
        .current_dir(&repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_repo() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("temp repo should be created");
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test User"]);
    git(dir.path(), &["commit", "--allow-empty", "-m", "initial"]);
    dir
}

#[tokio::test]
async fn branch_exists_strict_distinguishes_present_absent_and_invalid_refs() {
    let repo = setup_repo();
    git(repo.path(), &["branch", "feature/present"]);

    assert!(
        GitService::branch_exists_strict(repo.path(), "feature/present")
            .await
            .expect("present branch probe should succeed")
    );
    assert!(
        !GitService::branch_exists_strict(repo.path(), "feature/missing")
            .await
            .expect("missing branch probe should succeed")
    );
    let error = GitService::branch_exists_strict(repo.path(), "bad branch")
        .await
        .expect_err("invalid branch names should fail closed");
    assert!(matches!(error, AppError::Validation(_)));
}

#[tokio::test]
async fn create_worktree_strict_rejects_invalid_or_competing_branches_without_adopting() {
    let repo = setup_repo();
    let parent = tempfile::TempDir::new().expect("worktree parent should be created");
    let invalid_path = parent.path().join("invalid");
    let invalid =
        GitService::create_worktree_strict(repo.path(), &invalid_path, "bad branch", "main")
            .await
            .expect_err("invalid branch names should be rejected");
    assert!(matches!(invalid, AppError::Validation(_)));
    assert!(!invalid_path.exists());

    git(repo.path(), &["branch", "feature/existing"]);
    let competing_path = parent.path().join("existing");
    let competing = GitService::create_worktree_strict(
        repo.path(),
        &competing_path,
        "feature/existing",
        "main",
    )
    .await
    .expect_err("strict creation should not adopt an existing branch");
    assert!(matches!(competing, AppError::GitOperation(_)));
    assert!(!competing_path.exists());
}
