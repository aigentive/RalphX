use super::execution_complete_http;
use crate::application::AppState;
use crate::domain::entities::{Project, ProjectId, Task, TaskStep, ValidationCacheMetadata};
use crate::domain::ideation::TasksFeatureState;
use crate::http_server::types::{
    ExecutionCompleteRequest, HttpServerState, StartStepRequest, TestResultInput,
};
use crate::utils::path_safety::validate_absolute_non_root_path;
use axum::{
    extract::{Path, State},
    Json,
};
use std::sync::Arc;

use super::start_step_http;

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
    HttpServerState::new_test(app_state)
}

async fn task_for_repo(app_state: &AppState, repo_path: &std::path::Path, title: &str) -> Task {
    let mut project = Project::new(title.to_string(), repo_path.to_string_lossy().to_string());
    project.base_branch = Some("main".to_string());
    let project = app_state.project_repo.create(project).await.unwrap();
    Task::new(project.id.clone(), title.to_string())
}

#[tokio::test]
async fn start_step_preserves_tasks_disabled_error() {
    let app_state = Arc::new(AppState::new_test());
    app_state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Disabled,
            TasksFeatureState::Enabled,
        )
        .await
        .unwrap();

    let task = Task::new(
        ProjectId::from_string("tasks-disabled-steps".to_string()),
        "Task".to_string(),
    );
    app_state.task_repo.create(task.clone()).await.unwrap();
    let step = app_state
        .task_step_repo
        .create(TaskStep::new(
            task.id.clone(),
            "Step".to_string(),
            0,
            "agent".to_string(),
        ))
        .await
        .unwrap();

    app_state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Enabled,
            TasksFeatureState::Disabled,
        )
        .await
        .unwrap();

    let error = start_step_http(
        State(test_http_state(Arc::clone(&app_state))),
        Json(StartStepRequest {
            step_id: step.id.to_string(),
        }),
    )
    .await
    .expect_err("Tasks-off step mutation must be rejected");

    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|message| message.starts_with("ralphx:tasks_disabled")),
        "Tasks-off error must remain available to external callers"
    );
    assert_eq!(
        app_state
            .task_step_repo
            .get_by_id(&step.id)
            .await
            .unwrap()
            .expect("step remains present")
            .status,
        crate::domain::entities::TaskStepStatus::Pending
    );
}

async fn assert_execution_complete_rejects_tasks_feature_state(state: TasksFeatureState) {
    let app_state = Arc::new(AppState::new_test());
    app_state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Disabled,
            TasksFeatureState::Enabled,
        )
        .await
        .unwrap();

    let task = Task::new(
        ProjectId::from_string(format!("tasks-{state:?}-execution-complete")),
        "Task".to_string(),
    );
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    app_state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(TasksFeatureState::Enabled, state)
        .await
        .unwrap();

    let error = execution_complete_http(
        State(test_http_state(Arc::clone(&app_state))),
        Path(task_id.as_str().to_string()),
        Json(ExecutionCompleteRequest {
            summary: Some("done".to_string()),
            test_result: None,
        }),
    )
    .await
    .expect_err("Tasks-off execution completion must be rejected before side effects");

    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|message| message.starts_with("ralphx:tasks_disabled")),
        "Tasks-off error must remain available to external callers"
    );
    assert!(
        app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .is_some(),
        "rejection must preserve the task"
    );
}

#[tokio::test]
async fn execution_complete_rejects_while_tasks_disabled_before_side_effects() {
    assert_execution_complete_rejects_tasks_feature_state(TasksFeatureState::Disabled).await;
}

#[tokio::test]
async fn execution_complete_rejects_while_tasks_draining_before_side_effects() {
    assert_execution_complete_rejects_tasks_feature_state(TasksFeatureState::Draining).await;
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

    let mut task = task_for_repo(&app_state, tmp_dir.path(), "Dirty completion task").await;
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
        response
            .expect_err("dirty worktree must be rejected")
            .status,
        axum::http::StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn execution_complete_accepts_clean_committed_worktree() {
    let tmp_dir = create_temp_git_repo();
    let base_sha = git_stdout(tmp_dir.path(), &["rev-parse", "HEAD"]);
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

    let mut task = task_for_repo(&app_state, tmp_dir.path(), "Clean completion task").await;
    task.worktree_path = Some(tmp_dir.path().to_string_lossy().to_string());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(base_sha);
    let task_id = task.id.clone();
    let project_id = task.project_id.as_str().to_string();
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
    let events = app_state
        .external_events_repo
        .get_events_after_cursor(&[project_id], 0, 100)
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .all(|event| event.event_type != "task:execution_completed"),
        "execution_complete acknowledgement must not emit completion before the finalizer transition wins"
    );
}

#[tokio::test]
async fn execution_complete_rejects_empty_captured_base_diff() {
    let tmp_dir = create_temp_git_repo();
    let base_sha = git_stdout(tmp_dir.path(), &["rev-parse", "HEAD"]);

    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));

    let mut task = task_for_repo(&app_state, tmp_dir.path(), "Empty captured diff task").await;
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
        response
            .expect_err("empty captured-base diff must be rejected")
            .status,
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

    let mut task = task_for_repo(&app_state, tmp_dir.path(), "Non-empty captured diff task").await;
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
async fn execution_complete_does_not_accept_agent_test_result_as_validation_evidence() {
    let tmp_dir = create_temp_git_repo();
    let base_sha = git_stdout(tmp_dir.path(), &["rev-parse", "main"]);
    std::fs::write(tmp_dir.path().join("src.rs"), "fn main() {}\n").unwrap();
    run_git(tmp_dir.path(), &["add", "src.rs"]);
    run_git(tmp_dir.path(), &["commit", "-m", "task work"]);

    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));

    let mut task = task_for_repo(&app_state, tmp_dir.path(), "Validation cache task").await;
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
    assert!(
        ValidationCacheMetadata::from_task_metadata(updated.metadata.as_deref())
            .unwrap()
            .is_none(),
        "execution_complete must not convert agent-provided test_result into review evidence"
    );
}

#[tokio::test]
async fn execution_complete_rejects_failed_test_result_as_validation_failure() {
    let tmp_dir = create_temp_git_repo();
    let base_sha = git_stdout(tmp_dir.path(), &["rev-parse", "main"]);
    std::fs::write(tmp_dir.path().join("src.rs"), "fn main() {}\n").unwrap();
    run_git(tmp_dir.path(), &["add", "src.rs"]);
    run_git(tmp_dir.path(), &["commit", "-m", "task work"]);

    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));

    let mut task = task_for_repo(&app_state, tmp_dir.path(), "Red validation task").await;
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
                tests_passed: false,
                test_summary: Some("1 failed, 9 passed".to_string()),
            }),
        }),
    )
    .await;

    let error = response.expect_err("red validation must reject completion");
    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    let body = error.message.expect("validation rejection body");
    assert!(
        body.contains("\"error\":\"validation_failed\""),
        "error body should carry typed validation source: {body}"
    );

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    let metadata: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["failure_source"], "validation_failed");
    assert_eq!(
        metadata["failure_error"],
        "Validation failed: 1 failed, 9 passed"
    );
    assert!(
        ValidationCacheMetadata::from_task_metadata(updated.metadata.as_deref())
            .unwrap()
            .is_none(),
        "red validation must not be cached as green completion evidence"
    );
}
