use super::chat_service_merge::*;
use crate::application::runtime_factory::ChatRuntimeFactoryDeps;
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    InternalStatus, MergeStrategy, MergeValidationMode, Project, ReviewScopeMetadata, Task, TaskId,
};
use crate::domain::repositories::TaskRepository;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tokio::time::Duration;

fn run_git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("run git command");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_source_update_scope_drift_repo() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("create temp repo");
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "test@test.com"]);
    run_git(repo, &["config", "user.name", "Test"]);

    std::fs::write(repo.join("README.md"), "# test repo\n").expect("write readme");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial commit"]);

    run_git(repo, &["checkout", "-b", "task/scope-drift"]);
    std::fs::create_dir_all(repo.join("backend/app/services")).expect("create services dir");
    std::fs::write(
        repo.join("backend/app/services/applicability_evaluator.rb"),
        "class ApplicabilityEvaluator\nend\n",
    )
    .expect("write drift file");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "feat: out of scope drift"]);
    run_git(repo, &["checkout", "main"]);

    dir
}

async fn wait_for_status(
    task_repo: &Arc<dyn TaskRepository>,
    task_id: &TaskId,
    expected: InternalStatus,
) -> Task {
    let mut last = None;
    for _ in 0..50 {
        let task = task_repo
            .get_by_id(task_id)
            .await
            .expect("get task")
            .expect("task exists");
        if task.internal_status == expected {
            return task;
        }
        last = Some(task.internal_status);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    panic!(
        "task did not reach expected status {:?}; last state was {:?}",
        expected, last
    );
}

#[test]
fn build_pr_sync_services_includes_runtime_bound_pr_helpers() {
    let app_state = AppState::new_test();
    let plan_branch_repo = Some(Arc::clone(&app_state.plan_branch_repo));
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&app_state);

    let services = build_pr_sync_services_for_auto_complete(
        &app_state.task_repo,
        &plan_branch_repo,
        &app_state.ideation_session_repo,
        &app_state.artifact_repo,
        Some(&runtime_deps),
    );

    assert!(services.task_repo.is_some());
    assert!(services.plan_branch_repo.is_some());
    assert!(services.pr_creation_guard.is_some());
    assert!(services.ideation_session_repo.is_some());
    assert!(services.artifact_repo.is_some());
    assert!(services.plan_pr_description_drafter.is_some());
}

#[test]
fn build_pr_sync_services_without_runtime_deps_keeps_repo_helpers_only() {
    let app_state = AppState::new_test();
    let plan_branch_repo = Some(Arc::clone(&app_state.plan_branch_repo));

    let services = build_pr_sync_services_for_auto_complete(
        &app_state.task_repo,
        &plan_branch_repo,
        &app_state.ideation_session_repo,
        &app_state.artifact_repo,
        None,
    );

    assert!(services.task_repo.is_some());
    assert!(services.plan_branch_repo.is_some());
    assert!(services.pr_creation_guard.is_none());
    assert!(services.github_service.is_none());
    assert!(services.ideation_session_repo.is_some());
    assert!(services.artifact_repo.is_some());
    assert!(services.plan_pr_description_drafter.is_none());
}

#[test]
fn source_update_conflict_auto_complete_scope_drift_routes_to_reexecution() {
    // Linux coverage shards run this deep merge-retry regression on a
    // smaller worker stack than local macOS nextest runs.
    let handle = std::thread::Builder::new()
        .name("source-update-scope-drift-auto-complete".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build test runtime");
            runtime.block_on(
                source_update_conflict_auto_complete_scope_drift_routes_to_reexecution_body(),
            );
        })
        .expect("spawn stack-sized test thread");

    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}

async fn source_update_conflict_auto_complete_scope_drift_routes_to_reexecution_body() {
    let repo = setup_source_update_scope_drift_repo();
    let repo_path = repo.path();
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_max_concurrent(10);

    let mut project = Project::new(
        "Auto-complete scope drift project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project.merge_validation_mode = MergeValidationMode::Off;
    app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("create project");

    let mut task = Task::new(
        project.id.clone(),
        "Source update auto-complete should revise".to_string(),
    );
    task.internal_status = InternalStatus::Merging;
    task.task_branch = Some("task/scope-drift".to_string());
    task.worktree_path = Some(repo_path.to_string_lossy().to_string());
    let base_metadata = serde_json::json!({
        "source_update_conflict": true,
        "target_branch": "main",
        "source_branch": "task/scope-drift"
    })
    .to_string();
    task.metadata = Some(
        ReviewScopeMetadata::new(
            vec!["frontend/src".to_string()],
            Vec::new(),
            Some("unrelated_drift".to_string()),
            Some("backend service file was never classified during review".to_string()),
        )
        .update_task_metadata(Some(&base_metadata))
        .expect("scope metadata"),
    );
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.expect("create task");

    let plan_branch_repo = Some(Arc::clone(&app_state.plan_branch_repo));
    let interactive_process_registry = None;
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&app_state);
    let merge_ctx = MergeAutoCompleteContext {
        task_id_str: task_id.as_str(),
        task_id: task_id.clone(),
        task_repo: &app_state.task_repo,
        task_dependency_repo: &app_state.task_dependency_repo,
        project_repo: &app_state.project_repo,
        artifact_repo: &app_state.artifact_repo,
        chat_message_repo: &app_state.chat_message_repo,
        chat_attachment_repo: &app_state.chat_attachment_repo,
        conversation_repo: &app_state.chat_conversation_repo,
        agent_run_repo: &app_state.agent_run_repo,
        ideation_session_repo: &app_state.ideation_session_repo,
        activity_event_repo: &app_state.activity_event_repo,
        message_queue: &app_state.message_queue,
        running_agent_registry: &app_state.running_agent_registry,
        memory_event_repo: &app_state.memory_event_repo,
        execution_state: &execution_state,
        execution_settings_repo: Some(&app_state.execution_settings_repo),
        plan_branch_repo: &plan_branch_repo,
        events: &app_state.events,
        runtime_factory_deps: Some(&runtime_deps),
        interactive_process_registry: &interactive_process_registry,
    };

    attempt_merge_auto_complete(&merge_ctx).await;

    let updated =
        wait_for_status(&app_state.task_repo, &task_id, InternalStatus::ReExecuting).await;
    let metadata: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}"))
            .expect("task metadata is JSON");
    assert_eq!(metadata["error_code"], "merge_scope_drift_guard");
    assert_ne!(
        metadata["error_code"], "merge_scope_drift_guard_fallback",
        "auto-complete retry path must have TaskServices.transition_service available"
    );
}
