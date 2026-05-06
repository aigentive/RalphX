use super::execution_complete_http;
use crate::application::{AppState, TeamService, TeamStateTracker};
use crate::commands::ExecutionState;
use crate::domain::entities::{ProjectId, Task, ValidationCacheMetadata};
use crate::http_server::types::{ExecutionCompleteRequest, HttpServerState, TestResultInput};
use axum::{
    extract::{Path, State},
    Json,
};
use std::sync::Arc;

fn create_temp_git_repo() -> tempfile::TempDir {
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let repo_path = tmp_dir.path();
    let run_git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo_path)
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
    std::fs::write(repo_path.join("README.md"), "test").unwrap();
    run_git(&["add", "."]);
    run_git(&["commit", "-m", "init"]);

    tmp_dir
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
async fn execution_complete_writes_validation_cache_with_targeted_metadata_update() {
    let tmp_dir = create_temp_git_repo();
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));

    let mut task = Task::new(ProjectId::new(), "Validation cache task".to_string());
    task.worktree_path = Some(tmp_dir.path().to_string_lossy().to_string());
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
