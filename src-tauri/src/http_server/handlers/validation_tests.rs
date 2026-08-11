use std::path::Path;
use std::sync::Arc;

use axum::{extract::State, Json};

use super::{
    get_validation_task_diff_http, get_validation_task_diff_stat_http, ValidationTaskDiffRequest,
};
use crate::application::AppState;
use crate::domain::entities::{Project, Task};
use crate::http_server::types::HttpServerState;
use crate::utils::path_safety::validate_absolute_non_root_path;

fn run_git(repo_path: &Path, args: &[&str]) {
    let repo_path = validate_absolute_non_root_path(repo_path, "validation test git repository")
        .expect("safe test repo");
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

fn git_stdout(repo_path: &Path, args: &[&str]) -> String {
    let repo_path = validate_absolute_non_root_path(repo_path, "validation test git repository")
        .expect("safe test repo");
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

#[tokio::test]
async fn validation_task_diff_endpoints_ignore_request_base_when_task_base_is_immutable() {
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let repo_path = validate_absolute_non_root_path(
        tmp_dir.path(),
        "validation immutable base override test repo",
    )
    .expect("safe test repo");

    run_git(&repo_path, &["init", "-b", "main"]);
    run_git(&repo_path, &["config", "user.email", "test@test.com"]);
    run_git(&repo_path, &["config", "user.name", "Test"]);
    let readme_path = validate_absolute_non_root_path(&repo_path.join("README.md"), "test README")
        .expect("safe test README");
    std::fs::write(&readme_path, "base\n").expect("write base file");
    run_git(&repo_path, &["add", "README.md"]);
    run_git(&repo_path, &["commit", "-m", "base"]);
    let captured_base_sha = git_stdout(&repo_path, &["rev-parse", "HEAD"]);

    run_git(&repo_path, &["checkout", "-b", "task/change"]);
    std::fs::write(&readme_path, "base\nchange\n").expect("write task file");
    run_git(&repo_path, &["add", "README.md"]);
    run_git(&repo_path, &["commit", "-m", "task change"]);
    run_git(&repo_path, &["branch", "-f", "main", "HEAD"]);

    let app_state = Arc::new(AppState::new_test());
    let mut project = Project::new(
        "Validation Base Override".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    let project = app_state.project_repo.create(project).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Captured base task".to_string());
    task.worktree_path = Some(repo_path.to_string_lossy().to_string());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(captured_base_sha.clone());
    let task = app_state.task_repo.create(task).await.unwrap();
    let state = HttpServerState::new_test(Arc::clone(&app_state));

    let Json(stat_response) = get_validation_task_diff_stat_http(
        State(state.clone()),
        Json(ValidationTaskDiffRequest {
            task_id: task.id.as_str().to_string(),
            base_ref: Some("main".to_string()),
            file_paths: Vec::new(),
            max_files: None,
        }),
    )
    .await
    .expect("diff stat response");

    assert_eq!(stat_response.base_ref, captured_base_sha);
    assert_eq!(stat_response.display_base_ref, "main");
    assert!(stat_response.base_is_immutable);
    assert_eq!(stat_response.total_files, 1);
    assert_eq!(stat_response.files[0].path, "README.md");

    let Json(diff_response) = get_validation_task_diff_http(
        State(state),
        Json(ValidationTaskDiffRequest {
            task_id: task.id.as_str().to_string(),
            base_ref: Some("main".to_string()),
            file_paths: vec!["README.md".to_string()],
            max_files: Some(5),
        }),
    )
    .await
    .expect("diff response");

    assert_eq!(diff_response.base_ref, stat_response.base_ref);
    assert_eq!(diff_response.display_base_ref, "main");
    assert!(diff_response.base_is_immutable);
    assert_eq!(diff_response.files.len(), 1);
    assert_eq!(diff_response.files[0].path, "README.md");
    assert_eq!(diff_response.diffs.len(), 1);
}
