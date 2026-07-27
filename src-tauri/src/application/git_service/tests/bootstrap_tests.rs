use super::super::{GitBootstrapRequest, GitService};
use crate::infrastructure::tool_paths::resolve_git_cli_path;
use std::path::Path;
use std::process::Command;

fn git(path: &Path, args: &[&str]) -> std::process::Output {
    Command::new(resolve_git_cli_path())
        .args(args)
        .current_dir(path)
        .output()
        .expect("git command should run")
}

fn git_stdout(path: &Path, args: &[&str]) -> String {
    String::from_utf8(git(path, args).stdout)
        .expect("git stdout should be UTF-8")
        .trim()
        .to_string()
}

#[tokio::test]
async fn bootstrap_new_repository_creates_empty_root_without_user_identity() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    std::fs::write(directory.path().join("README.md"), "untracked user file\n")
        .expect("fixture file should write");

    let bootstrap = GitService::bootstrap_project_repository(
        directory.path(),
        GitBootstrapRequest::new(Some("main".to_string())),
    )
    .await
    .expect("bootstrap should create a usable repository");

    assert_eq!(bootstrap.base_branch, "main");
    assert_eq!(
        git_stdout(directory.path(), &["symbolic-ref", "--short", "HEAD"]),
        "main"
    );
    assert_eq!(
        git_stdout(directory.path(), &["rev-parse", "--verify", "HEAD"]).len(),
        40
    );
    assert_eq!(
        git_stdout(directory.path(), &["show", "-s", "--format=%T", "HEAD"]),
        git_stdout(directory.path(), &["mktree"])
    );
    assert_eq!(
        git_stdout(directory.path(), &["show", "-s", "--format=%an", "HEAD"]),
        "RalphX"
    );
    assert_eq!(
        git_stdout(directory.path(), &["show", "-s", "--format=%ae", "HEAD"]),
        "ralphx@localhost"
    );
    assert!(!git(
        directory.path(),
        &["config", "--local", "--get", "user.name"]
    )
    .status
    .success());
    assert!(!git(
        directory.path(),
        &["config", "--local", "--get", "user.email"]
    )
    .status
    .success());
    assert!(directory.path().join("README.md").is_file());
}

#[tokio::test]
async fn bootstrap_nested_child_initializes_its_own_repository_instead_of_using_parent() {
    let parent = tempfile::tempdir().expect("parent directory should exist");
    assert!(git(parent.path(), &["init", "--initial-branch", "main"])
        .status
        .success());
    let child = parent.path().join("child-project");
    std::fs::create_dir(&child).expect("child project directory should exist");

    let bootstrap = GitService::bootstrap_project_repository(
        &child,
        GitBootstrapRequest::new(Some("develop".to_string())),
    )
    .await
    .expect("nested child should bootstrap its own repository");

    assert_eq!(bootstrap.base_branch, "develop");
    let canonical_child = child
        .canonicalize()
        .expect("child project directory should canonicalize");
    assert_eq!(
        git_stdout(&child, &["rev-parse", "--show-toplevel"]),
        canonical_child.to_string_lossy()
    );
    assert_eq!(
        git_stdout(&child, &["symbolic-ref", "--short", "HEAD"]),
        "develop"
    );
    assert!(
        !git(
            parent.path(),
            &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]
        )
        .status
        .success(),
        "bootstrap must not create history in the parent repository"
    );
}

#[tokio::test]
async fn bootstrap_unborn_repository_preserves_symbolic_branch_and_staged_user_files() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    assert!(
        git(directory.path(), &["init", "--initial-branch", "develop"])
            .status
            .success()
    );
    std::fs::write(directory.path().join("staged.txt"), "do not consume me\n")
        .expect("fixture file should write");
    assert!(git(directory.path(), &["add", "staged.txt"])
        .status
        .success());

    let bootstrap = GitService::bootstrap_project_repository(
        directory.path(),
        GitBootstrapRequest::new(Some("main".to_string())),
    )
    .await
    .expect("unborn repository should bootstrap");

    assert_eq!(bootstrap.base_branch, "develop");
    assert_eq!(
        git_stdout(directory.path(), &["symbolic-ref", "--short", "HEAD"]),
        "develop"
    );
    assert_eq!(
        git_stdout(directory.path(), &["diff", "--cached", "--name-only"]),
        "staged.txt"
    );
    assert!(git_stdout(
        directory.path(),
        &["show", "--format=", "--name-only", "HEAD"]
    )
    .is_empty());
}

#[tokio::test]
async fn bootstrap_existing_repository_never_rewrites_history_and_requires_selected_base() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    assert!(git(directory.path(), &["init", "--initial-branch", "main"])
        .status
        .success());
    assert!(git(directory.path(), &["config", "user.name", "User Name"])
        .status
        .success());
    assert!(git(
        directory.path(),
        &["config", "user.email", "user@example.com"]
    )
    .status
    .success());
    std::fs::write(directory.path().join("README.md"), "base\n").expect("fixture should write");
    assert!(git(directory.path(), &["add", "README.md"])
        .status
        .success());
    assert!(git(directory.path(), &["commit", "-m", "existing history"])
        .status
        .success());
    let head_before = git_stdout(directory.path(), &["rev-parse", "HEAD"]);

    let bootstrap = GitService::bootstrap_project_repository(
        directory.path(),
        GitBootstrapRequest::new(Some("main".to_string())),
    )
    .await
    .expect("existing repository should validate");
    assert_eq!(bootstrap.base_branch, "main");
    assert_eq!(
        git_stdout(directory.path(), &["rev-parse", "HEAD"]),
        head_before
    );

    let error = GitService::bootstrap_project_repository(
        directory.path(),
        GitBootstrapRequest::new(Some("missing".to_string())),
    )
    .await
    .expect_err("missing selected base must reject persistence");
    assert!(error.to_string().contains("missing"));
    assert_eq!(
        git_stdout(directory.path(), &["rev-parse", "HEAD"]),
        head_before
    );
}

#[tokio::test]
async fn bootstrap_rejects_detached_existing_repository() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    assert!(git(directory.path(), &["init", "--initial-branch", "main"])
        .status
        .success());
    assert!(git(directory.path(), &["config", "user.name", "User Name"])
        .status
        .success());
    assert!(git(
        directory.path(),
        &["config", "user.email", "user@example.com"]
    )
    .status
    .success());
    std::fs::write(directory.path().join("README.md"), "base\n").expect("fixture should write");
    assert!(git(directory.path(), &["add", "README.md"])
        .status
        .success());
    assert!(git(directory.path(), &["commit", "-m", "base"])
        .status
        .success());
    assert!(git(directory.path(), &["checkout", "--detach", "HEAD"])
        .status
        .success());

    let error =
        GitService::bootstrap_project_repository(directory.path(), GitBootstrapRequest::new(None))
            .await
            .expect_err("detached repository must reject bootstrap");

    assert!(error.to_string().contains("symbolic branch"));
}

#[tokio::test]
async fn bootstrap_rejects_invalid_existing_git_metadata_without_repairing_it() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let git_dir = directory.path().join(".git");
    std::fs::create_dir(&git_dir).expect("invalid git metadata directory should exist");
    let head_path = git_dir.join("HEAD");
    std::fs::write(&head_path, "not a valid git head\n").expect("invalid HEAD should write");

    let error = GitService::bootstrap_project_repository(
        directory.path(),
        GitBootstrapRequest::new(Some("main".to_string())),
    )
    .await
    .expect_err("invalid git metadata must reject bootstrap");

    assert!(error.to_string().contains("Git metadata"));
    assert_eq!(
        std::fs::read_to_string(&head_path).expect("HEAD must be preserved"),
        "not a valid git head\n"
    );
    assert!(
        !git_dir.join("config").exists(),
        "strict bootstrap must not repair invalid metadata"
    );
}

#[tokio::test]
async fn bootstrap_rejects_invalid_child_metadata_without_falling_back_to_parent_repository() {
    let parent = tempfile::tempdir().expect("parent directory should exist");
    assert!(git(parent.path(), &["init", "--initial-branch", "main"])
        .status
        .success());
    let child = parent.path().join("invalid-child-project");
    std::fs::create_dir(&child).expect("child project directory should exist");
    let git_dir = child.join(".git");
    std::fs::create_dir(&git_dir).expect("invalid Git metadata directory should exist");
    let head_path = git_dir.join("HEAD");
    std::fs::write(&head_path, "not a valid git head\n").expect("invalid HEAD should write");

    let error = GitService::bootstrap_project_repository(
        &child,
        GitBootstrapRequest::new(Some("main".to_string())),
    )
    .await
    .expect_err("invalid child metadata must reject bootstrap");

    assert!(error.to_string().contains("Git metadata"));
    assert_eq!(
        std::fs::read_to_string(&head_path).expect("child HEAD must be preserved"),
        "not a valid git head\n"
    );
    assert!(
        !git(
            parent.path(),
            &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]
        )
        .status
        .success(),
        "invalid child metadata must not create history in the parent repository"
    );
}
