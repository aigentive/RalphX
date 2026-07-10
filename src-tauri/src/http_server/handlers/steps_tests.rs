use super::execution_complete_http;
use crate::application::{AppState, TeamService, TeamStateTracker};
use crate::commands::ExecutionState;
use crate::domain::entities::{ProjectId, Task, ValidationCacheMetadata};
use crate::http_server::types::{ExecutionCompleteRequest, HttpServerState, TestResultInput};
use crate::utils::path_safety::validate_absolute_non_root_path;
use axum::{
    extract::{Path, State},
    Json,
};
use std::sync::Arc;

fn create_temp_git_repo() -> tempfile::TempDir {
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let repo_path = validate_absolute_non_root_path(tmp_dir.path(), "test git repository")
        .expect("safe test repo");
    let run_git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo_path)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };

    run_git(&["init", "-b", "main"]);
    run_git(&["config", "user.email", "test@test.com"]);
    run_git(&["config", "user.name", "Test"]);
    let readme_path =
        validate_absolute_non_root_path(&repo_path.join("README.md"), "test repository README")
            .unwrap();
    std::fs::write(readme_path, "test").unwrap();
    run_git(&["add", "."]);
    run_git(&["commit", "-m", "init"]);

    tmp_dir
}

fn run_git(repo_path: &std::path::Path, args: &[&str]) {
    let repo_path =
        validate_absolute_non_root_path(repo_path, "test git repository").expect("safe test repo");
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(&repo_path)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .expect("git command failed");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(repo_path: &std::path::Path, args: &[&str]) -> String {
    let repo_path =
        validate_absolute_non_root_path(repo_path, "test git repository").expect("safe test repo");
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(&repo_path)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .expect("git command failed");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn test_http_state(app_state: Arc<AppState>) -> HttpServerState {
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
    HttpServerState {
        app_state,
        execution_state: Arc::new(ExecutionState::new()),
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

#[tokio::test]
async fn execution_complete_rejects_dirty_worktree() {
    let tmp_dir = create_temp_git_repo();
    let source_path = validate_absolute_non_root_path(
        &tmp_dir.path().join("src.rs"),
        "dirty completion test source",
    )
    .unwrap();
    std::fs::write(source_path, "uncommitted").unwrap();

    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));

    let mut task = Task::new(ProjectId::new(), "Dirty completion task".to_string());
    task.worktree_path = Some(tmp_dir.path().to_string_lossy().to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let response = execution_complete_http(
        State(state),
        Path(task_id.as_str().to_string()),
        Json(ExecutionCompleteRequest {
            summary: Some("done".to_string()),
            test_result: None,
        }),
    )
    .await;

    assert_eq!(
        response.expect_err("dirty worktree must be rejected"),
        axum::http::StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn execution_complete_accepts_clean_committed_worktree() {
    let tmp_dir = create_temp_git_repo();
    let source_path = validate_absolute_non_root_path(
        &tmp_dir.path().join("src.rs"),
        "clean completion test source",
    )
    .unwrap();
    std::fs::write(source_path, "committed").unwrap();
    run_git(tmp_dir.path(), &["add", "src.rs"]);
    run_git(tmp_dir.path(), &["commit", "-m", "task work"]);

    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));

    let mut task = Task::new(ProjectId::new(), "Clean completion task".to_string());
    task.worktree_path = Some(tmp_dir.path().to_string_lossy().to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let response = execution_complete_http(
        State(state),
        Path(task_id.as_str().to_string()),
        Json(ExecutionCompleteRequest {
            summary: Some("done".to_string()),
            test_result: None,
        }),
    )
    .await
    .unwrap();

    assert!(response.0.success);
}

#[tokio::test]
async fn execution_complete_rejects_empty_captured_base_diff() {
    let tmp_dir = create_temp_git_repo();
    let base_sha = git_stdout(tmp_dir.path(), &["rev-parse", "HEAD"]);

    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));

    let mut task = Task::new(ProjectId::new(), "Empty captured diff task".to_string());
    task.worktree_path = Some(tmp_dir.path().to_string_lossy().to_string());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(base_sha);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let response = execution_complete_http(
        State(state),
        Path(task_id.as_str().to_string()),
        Json(ExecutionCompleteRequest {
            summary: Some("done".to_string()),
            test_result: None,
        }),
    )
    .await;

    assert_eq!(
        response.expect_err("empty captured-base diff must be rejected"),
        axum::http::StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn execution_complete_accepts_non_empty_captured_base_diff() {
    let tmp_dir = create_temp_git_repo();
    let base_sha = git_stdout(tmp_dir.path(), &["rev-parse", "HEAD"]);
    let source_path = validate_absolute_non_root_path(
        &tmp_dir.path().join("src.rs"),
        "captured completion test source",
    )
    .unwrap();
    std::fs::write(source_path, "committed").unwrap();
    run_git(tmp_dir.path(), &["add", "src.rs"]);
    run_git(tmp_dir.path(), &["commit", "-m", "task work"]);

    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));

    let mut task = Task::new(ProjectId::new(), "Non-empty captured diff task".to_string());
    task.worktree_path = Some(tmp_dir.path().to_string_lossy().to_string());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(base_sha);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let response = execution_complete_http(
        State(state),
        Path(task_id.as_str().to_string()),
        Json(ExecutionCompleteRequest {
            summary: Some("done".to_string()),
            test_result: None,
        }),
    )
    .await
    .unwrap();

    assert!(response.0.success);
}

#[tokio::test]
async fn execution_complete_writes_validation_cache_with_targeted_metadata_update() {
    let tmp_dir = create_temp_git_repo();
    let base_sha = git_stdout(tmp_dir.path(), &["rev-parse", "main"]);
    std::fs::write(tmp_dir.path().join("src.rs"), "fn main() {}\n").unwrap();
    run_git(tmp_dir.path(), &["add", "src.rs"]);
    run_git(tmp_dir.path(), &["commit", "-m", "task work"]);

    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));

    let mut task = Task::new(ProjectId::new(), "Validation cache task".to_string());
    task.worktree_path = Some(tmp_dir.path().to_string_lossy().to_string());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(base_sha);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let response = execution_complete_http(
        State(state),
        Path(task_id.as_str().to_string()),
        Json(ExecutionCompleteRequest {
            summary: Some("done".to_string()),
            test_result: Some(TestResultInput {
                tests_ran: true,
                tests_passed: true,
                test_summary: Some("focused validation passed".to_string()),
            }),
        }),
    )
    .await
    .unwrap();

    assert!(response.0.success);

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    let cache = ValidationCacheMetadata::from_task_metadata(updated.metadata.as_deref())
        .unwrap()
        .expect("execution_complete should store validation cache metadata");
    assert!(cache.tests_ran);
    assert!(cache.tests_passed);
    assert_eq!(
        cache.test_summary.as_deref(),
        Some("focused validation passed")
    );
    assert_eq!(cache.captured_by, "execution_complete");
}
