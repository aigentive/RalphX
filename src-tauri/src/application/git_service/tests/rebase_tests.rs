use std::process::Command;

use super::super::*;
use crate::error::AppError;

fn init_repository() -> tempfile::TempDir {
    let repository = tempfile::tempdir().expect("repository should be created");
    let output = Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repository.path())
        .output()
        .expect("git init should run");
    assert!(output.status.success(), "git init should succeed");
    repository
}

#[tokio::test]
async fn restart_strict_origin_fetch_rejects_invalid_branch_and_missing_origin() {
    let repository = init_repository();

    let invalid_branch = GitService::fetch_origin_branch_strict(repository.path(), "bad..branch")
        .await
        .expect_err("invalid branch should not reach git fetch");
    assert!(matches!(invalid_branch, AppError::Validation(_)));

    let missing_origin = GitService::fetch_origin_branch_strict(repository.path(), "main")
        .await
        .expect_err("restart should require an origin remote");
    assert!(matches!(missing_origin, AppError::GitOperation(_)));
}
