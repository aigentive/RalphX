use crate::support::real_git_repo::setup_real_git_repo;
use axum::{
    extract::{Path, State},
    Json,
};
use ralphx_lib::application::{AppState, InteractiveProcessKey};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    IdeationSession, InternalStatus, Priority, Project, ProjectId, ProposalCategory,
    ScopeDriftStatus, Task, TaskProposal, TaskStep,
};
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::helpers::get_task_context_impl;
use ralphx_lib::http_server::project_scope::ProjectScope;
use ralphx_lib::http_server::types::{ExecutionCompleteRequest, HttpServerState, TestResultInput};
use std::sync::Arc;

async fn setup_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    let execution_state = Arc::new(ExecutionState::new());

    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
        external_mcp_supervisor: None,
    }
}

async fn create_test_project(state: &HttpServerState, name: &str, path: &str) -> Project {
    let project = Project::new(name.to_string(), path.to_string());
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    project
}

fn mark_no_code_changes(task: &mut Task) {
    task.metadata = Some(serde_json::json!({ "no_code_changes": true }).to_string());
}

#[tokio::test]
async fn test_add_step_with_parent() {
    let state = setup_test_state().await;

    // Create a project
    let project_id = ProjectId::new();

    // Create a task
    let task = Task::new(project_id, "Test Task".to_string());
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    // Create a parent step
    let parent_req = AddStepRequest {
        task_id: task_id.as_str().to_string(),
        title: "Parent Step".to_string(),
        description: None,
        after_step_id: None,
        parent_step_id: None,
        scope_context: None,
    };
    let parent_response = add_step_http(State(state.clone()), Json(parent_req))
        .await
        .unwrap();
    let parent_id = parent_response.0.id.clone();

    // Create a sub-step
    let sub_req = AddStepRequest {
        task_id: task_id.as_str().to_string(),
        title: "Sub Step".to_string(),
        description: Some("A sub-step".to_string()),
        after_step_id: None,
        parent_step_id: Some(parent_id.clone()),
        scope_context: Some(r#"{"files":["test.rs"]}"#.to_string()),
    };

    let response = add_step_http(State(state.clone()), Json(sub_req))
        .await
        .unwrap();

    assert_eq!(response.0.parent_step_id, Some(parent_id));
    assert_eq!(
        response.0.scope_context,
        Some(r#"{"files":["test.rs"]}"#.to_string())
    );
}

#[tokio::test]
async fn test_get_step_context() {
    let state = setup_test_state().await;

    // Create a project
    let project_id = ProjectId::new();

    // Create a task
    let task = Task::new(project_id, "Test Task".to_string());
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    // Create parent and sub-steps
    let parent_step = TaskStep::new(task_id.clone(), "Parent".to_string(), 0, "test".to_string());
    let parent_id = parent_step.id.clone();
    state
        .app_state
        .task_step_repo
        .create(parent_step)
        .await
        .unwrap();

    let mut sub_step = TaskStep::new(task_id.clone(), "Sub".to_string(), 0, "test".to_string());
    sub_step.parent_step_id = Some(parent_id.clone());
    sub_step.scope_context = Some(r#"{"files":["test.rs"]}"#.to_string());
    let sub_id = sub_step.id.clone();
    state
        .app_state
        .task_step_repo
        .create(sub_step)
        .await
        .unwrap();

    // Get step context
    let response = get_step_context_http(State(state.clone()), Path(sub_id.as_str().to_string()))
        .await
        .unwrap();

    assert_eq!(response.0.step.id, sub_id.as_str());
    assert_eq!(response.0.parent_step.unwrap().id, parent_id.as_str());
    assert_eq!(response.0.task_summary.id, task_id.as_str());
    assert!(response.0.scope_context.is_some());
    assert!(!response.0.context_hints.is_empty());
}

#[tokio::test]
async fn test_get_sub_steps() {
    let state = setup_test_state().await;

    // Create a project
    let project_id = ProjectId::new();

    // Create a task
    let task = Task::new(project_id, "Test Task".to_string());
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    // Create parent step
    let parent_step = TaskStep::new(task_id.clone(), "Parent".to_string(), 0, "test".to_string());
    let parent_id = parent_step.id.clone();
    state
        .app_state
        .task_step_repo
        .create(parent_step)
        .await
        .unwrap();

    // Create 2 sub-steps
    let mut sub1 = TaskStep::new(task_id.clone(), "Sub 1".to_string(), 0, "test".to_string());
    sub1.parent_step_id = Some(parent_id.clone());
    state.app_state.task_step_repo.create(sub1).await.unwrap();

    let mut sub2 = TaskStep::new(task_id.clone(), "Sub 2".to_string(), 1, "test".to_string());
    sub2.parent_step_id = Some(parent_id.clone());
    state.app_state.task_step_repo.create(sub2).await.unwrap();

    // Get sub-steps
    let response = get_sub_steps_http(State(state.clone()), Path(parent_id.as_str().to_string()))
        .await
        .unwrap();

    assert_eq!(response.0.len(), 2);
    assert_eq!(response.0[0].title, "Sub 1");
    assert_eq!(response.0[1].title, "Sub 2");
}

#[tokio::test]
async fn test_get_task_context_reports_scope_expansion_against_proposal_scope() {
    let state = AppState::new_sqlite_test();
    let repo = setup_real_git_repo();

    let mut project = Project::new("Scope Drift Project".to_string(), repo.path_string());
    project.base_branch = Some("main".to_string());
    let project_id = project.id.clone();
    state.project_repo.create(project).await.unwrap();

    let session = IdeationSession::new_with_title(project_id.clone(), "Scoped Session");
    let session_id = session.id.clone();
    state.ideation_session_repo.create(session).await.unwrap();

    let mut proposal = TaskProposal::new(
        session_id.clone(),
        "Scoped proposal",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    proposal.affected_paths =
        Some(serde_json::to_string(&vec!["src-tauri/src/http_server".to_string()]).unwrap());
    let proposal_id = proposal.id.clone();
    state.task_proposal_repo.create(proposal).await.unwrap();

    let mut task = Task::new(project_id, "Scoped execution task".to_string());
    task.source_proposal_id = Some(proposal_id);
    task.ideation_session_id = Some(session_id);
    task.task_branch = Some(repo.task_branch.clone());
    state.task_repo.create(task.clone()).await.unwrap();

    let checkout_status = std::process::Command::new("git")
        .args(["checkout", &repo.task_branch])
        .current_dir(repo.path())
        .status()
        .expect("checkout task branch");
    assert!(
        checkout_status.success(),
        "task branch checkout must succeed"
    );

    let context = get_task_context_impl(&state, &task.id).await.unwrap();

    assert_eq!(context.scope_drift_status, ScopeDriftStatus::ScopeExpansion);
    assert!(
        context
            .actual_changed_files
            .iter()
            .any(|path| path == "feature.rs"),
        "expected actual changed files to include feature.rs, got {:?}",
        context.actual_changed_files
    );
    assert_eq!(context.out_of_scope_files, vec!["feature.rs".to_string()]);
    assert_eq!(
        context.source_proposal.unwrap().affected_paths,
        vec!["src-tauri/src/http_server".to_string()]
    );
}

// ============================================================================
// IPR (Interactive Process Registry) exit signal tests
// ============================================================================

/// Helper: spawn a cat process to get a live ChildStdin for IPR registration.
/// The caller is responsible for killing the child after the test.
async fn spawn_test_stdin() -> (tokio::process::Child, tokio::process::ChildStdin) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn cat for IPR test");
    let stdin = child.stdin.take().expect("cat stdin handle");
    (child, stdin)
}

/// execution_complete happy path: task exists, IPR entry registered → handler removes IPR.
#[tokio::test]
async fn test_execution_complete_removes_ipr_entry() {
    let state = setup_test_state().await;

    let project = create_test_project(&state, "Exec complete project", "/tmp/exec-complete").await;
    let mut task = Task::new(project.id, "Exec complete test".to_string());
    mark_no_code_changes(&mut task);
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    // Register a live IPR entry
    let (mut child, stdin) = spawn_test_stdin().await;
    let key = InteractiveProcessKey::new("task_execution", task_id.as_str());
    state
        .app_state
        .interactive_process_registry
        .register(key.clone(), stdin)
        .await;

    assert!(
        state
            .app_state
            .interactive_process_registry
            .has_process(&key)
            .await,
        "IPR entry should be present before handler call"
    );

    let req = ExecutionCompleteRequest {
        summary: Some("All done".to_string()),
        test_result: None,
    };
    let result = execution_complete_http(
        State(state.clone()),
        Path(task_id.as_str().to_string()),
        Json(req),
    )
    .await;

    assert!(result.is_ok(), "handler should succeed: {:?}", result.err());
    assert!(
        result.unwrap().0.success,
        "response success flag must be true"
    );

    assert!(
        !state
            .app_state
            .interactive_process_registry
            .has_process(&key)
            .await,
        "IPR entry must be removed after execution_complete"
    );

    let _ = child.kill().await;
}

/// execution_complete with non-existent task_id returns 404.
#[tokio::test]
async fn test_execution_complete_task_not_found_returns_404() {
    let state = setup_test_state().await;

    let result = execution_complete_http(
        State(state.clone()),
        Path("non-existent-task-id".to_string()),
        Json(ExecutionCompleteRequest {
            summary: None,
            test_result: None,
        }),
    )
    .await;

    match result {
        Err(error) => assert_eq!(
            error.status,
            axum::http::StatusCode::NOT_FOUND,
            "expected 404 for non-existent task"
        ),
        Ok(_) => panic!("expected 404 for non-existent task"),
    }
}

/// execution_complete without an IPR entry succeeds (agent already exited or never registered).
#[tokio::test]
async fn test_execution_complete_no_ipr_entry_is_idempotent() {
    let state = setup_test_state().await;

    let project = create_test_project(&state, "No IPR project", "/tmp/no-ipr").await;
    let mut task = Task::new(project.id, "No IPR test".to_string());
    mark_no_code_changes(&mut task);
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    // No IPR entry registered — handler should still succeed
    let result = execution_complete_http(
        State(state.clone()),
        Path(task_id.as_str().to_string()),
        Json(ExecutionCompleteRequest {
            summary: None,
            test_result: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "handler must succeed even without IPR entry: {:?}",
        result.err()
    );
}

/// execution_complete called twice: second call also succeeds (idempotent).
#[tokio::test]
async fn test_execution_complete_double_call_idempotent() {
    let state = setup_test_state().await;

    let project = create_test_project(&state, "Double call project", "/tmp/double-call").await;
    let mut task = Task::new(project.id, "Double call test".to_string());
    mark_no_code_changes(&mut task);
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    let (mut child, stdin) = spawn_test_stdin().await;
    let key = InteractiveProcessKey::new("task_execution", task_id.as_str());
    state
        .app_state
        .interactive_process_registry
        .register(key.clone(), stdin)
        .await;

    // First call removes the IPR entry
    let result1 = execution_complete_http(
        State(state.clone()),
        Path(task_id.as_str().to_string()),
        Json(ExecutionCompleteRequest {
            summary: None,
            test_result: None,
        }),
    )
    .await;
    assert!(result1.is_ok(), "first call should succeed");

    // Second call — IPR already removed, handler must still return Ok
    let result2 = execution_complete_http(
        State(state.clone()),
        Path(task_id.as_str().to_string()),
        Json(ExecutionCompleteRequest {
            summary: None,
            test_result: None,
        }),
    )
    .await;
    assert!(
        result2.is_ok(),
        "second call should also succeed (idempotent)"
    );

    let _ = child.kill().await;
}

/// complete_step with all steps done triggers the all-steps-done IPR close fallback.
///
/// Scenario: Worker completes the final step → handler detects all steps are Completed/Skipped
/// → removes IPR entry so the agent receives EOF and exits gracefully.
#[tokio::test]
async fn test_complete_step_all_done_removes_ipr() {
    let state = setup_test_state().await;

    let project_id = ProjectId::new();
    let task = Task::new(project_id, "All steps done test".to_string());
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    // Create 2 steps (Pending by default)
    let step1_resp = add_step_http(
        State(state.clone()),
        Json(AddStepRequest {
            task_id: task_id.as_str().to_string(),
            title: "Step 1".to_string(),
            description: None,
            after_step_id: None,
            parent_step_id: None,
            scope_context: None,
        }),
    )
    .await
    .unwrap();
    let step1_id = step1_resp.0.id.clone();

    let step2_resp = add_step_http(
        State(state.clone()),
        Json(AddStepRequest {
            task_id: task_id.as_str().to_string(),
            title: "Step 2".to_string(),
            description: None,
            after_step_id: None,
            parent_step_id: None,
            scope_context: None,
        }),
    )
    .await
    .unwrap();
    let step2_id = step2_resp.0.id.clone();

    // Transition both steps Pending → InProgress via start_step_http
    let _ = start_step_http(
        State(state.clone()),
        Json(StartStepRequest {
            step_id: step1_id.clone(),
        }),
    )
    .await
    .unwrap();
    let _ = start_step_http(
        State(state.clone()),
        Json(StartStepRequest {
            step_id: step2_id.clone(),
        }),
    )
    .await
    .unwrap();

    // Register IPR entry for the worker
    let (mut child, stdin) = spawn_test_stdin().await;
    let key = InteractiveProcessKey::new("task_execution", task_id.as_str());
    state
        .app_state
        .interactive_process_registry
        .register(key.clone(), stdin)
        .await;

    // Complete step 1 — step 2 still InProgress → IPR must remain
    let _ = complete_step_http(
        State(state.clone()),
        Json(CompleteStepRequest {
            step_id: step1_id,
            note: None,
        }),
    )
    .await
    .unwrap();

    assert!(
        state
            .app_state
            .interactive_process_registry
            .has_process(&key)
            .await,
        "IPR must remain after completing only 1 of 2 steps"
    );

    // Complete step 2 — all steps now Completed → fallback removes IPR
    let _ = complete_step_http(
        State(state.clone()),
        Json(CompleteStepRequest {
            step_id: step2_id,
            note: None,
        }),
    )
    .await
    .unwrap();

    assert!(
        !state
            .app_state
            .interactive_process_registry
            .has_process(&key)
            .await,
        "IPR must be removed after all steps are completed (all-steps-done fallback)"
    );

    let _ = child.kill().await;
}

/// skip_step with all steps done triggers the all-steps-done IPR close fallback.
///
/// Scenario: Worker skips the final step → handler detects all steps are Completed/Skipped
/// → removes IPR entry so the agent receives EOF and exits gracefully.
#[tokio::test]
async fn test_skip_step_all_done_removes_ipr() {
    let state = setup_test_state().await;

    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Skip all steps done test".to_string());
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    // Create 2 steps (Pending by default)
    let step1_resp = add_step_http(
        State(state.clone()),
        Json(AddStepRequest {
            task_id: task_id.as_str().to_string(),
            title: "Step 1".to_string(),
            description: None,
            after_step_id: None,
            parent_step_id: None,
            scope_context: None,
        }),
    )
    .await
    .unwrap();
    let step1_id = step1_resp.0.id.clone();

    let step2_resp = add_step_http(
        State(state.clone()),
        Json(AddStepRequest {
            task_id: task_id.as_str().to_string(),
            title: "Step 2".to_string(),
            description: None,
            after_step_id: None,
            parent_step_id: None,
            scope_context: None,
        }),
    )
    .await
    .unwrap();
    let step2_id = step2_resp.0.id.clone();

    // Transition step 1 Pending → InProgress so it can be completed
    let _ = start_step_http(
        State(state.clone()),
        Json(StartStepRequest {
            step_id: step1_id.clone(),
        }),
    )
    .await
    .unwrap();

    // Register IPR entry for the worker
    let (mut child, stdin) = spawn_test_stdin().await;
    let key = InteractiveProcessKey::new("task_execution", task_id.as_str());
    state
        .app_state
        .interactive_process_registry
        .register(key.clone(), stdin)
        .await;

    // Complete step 1 — step 2 still Pending → IPR must remain
    let _ = complete_step_http(
        State(state.clone()),
        Json(CompleteStepRequest {
            step_id: step1_id,
            note: None,
        }),
    )
    .await
    .unwrap();

    assert!(
        state
            .app_state
            .interactive_process_registry
            .has_process(&key)
            .await,
        "IPR must remain after completing only 1 of 2 steps"
    );

    // Skip step 2 (still Pending — skip is valid from Pending state)
    // → all steps now Completed/Skipped → fallback removes IPR
    let _ = skip_step_http(
        State(state.clone()),
        Json(SkipStepRequest {
            step_id: step2_id,
            reason: "Not needed for this implementation".to_string(),
        }),
    )
    .await
    .unwrap();

    assert!(
        !state
            .app_state
            .interactive_process_registry
            .has_process(&key)
            .await,
        "IPR must be removed after all steps done via skip (all-steps-done fallback)"
    );

    let _ = child.kill().await;
}

// ============================================================================
// get_task_steps_http scope guard tests
// ============================================================================

/// get_task_steps_http — no scope header (internal agent) returns steps without checking project.
#[tokio::test]
async fn test_get_task_steps_no_scope_header_returns_steps() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();
    let task = Task::new(project_id.clone(), "Steps task".to_string());
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    // Create 2 steps
    state
        .app_state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Step 1".to_string(),
            0,
            "test".to_string(),
        ))
        .await
        .unwrap();
    state
        .app_state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Step 2".to_string(),
            1,
            "test".to_string(),
        ))
        .await
        .unwrap();

    // No scope header → ProjectScope(None) → unrestricted
    let result = get_task_steps_http(
        State(state.clone()),
        ProjectScope(None),
        Path(task_id.as_str().to_string()),
    )
    .await
    .unwrap();

    assert_eq!(result.0.len(), 2);
    assert_eq!(result.0[0].title, "Step 1");
    assert_eq!(result.0[1].title, "Step 2");
}

/// get_task_steps_http — no steps returns empty list.
#[tokio::test]
async fn test_get_task_steps_empty_returns_empty_list() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();
    let task = Task::new(project_id.clone(), "Empty steps task".to_string());
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    let result = get_task_steps_http(
        State(state.clone()),
        ProjectScope(None),
        Path(task_id.as_str().to_string()),
    )
    .await
    .unwrap();

    assert!(
        result.0.is_empty(),
        "expected empty list when no steps exist"
    );
}

/// get_task_steps_http — scope header present with matching project ID returns steps.
#[tokio::test]
async fn test_get_task_steps_scope_header_matching_project_returns_steps() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();
    let task = Task::new(project_id.clone(), "Scoped task".to_string());
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    state
        .app_state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Scoped step".to_string(),
            0,
            "test".to_string(),
        ))
        .await
        .unwrap();

    // Scope header present, project_id matches → allowed
    let result = get_task_steps_http(
        State(state.clone()),
        ProjectScope(Some(vec![project_id.clone()])),
        Path(task_id.as_str().to_string()),
    )
    .await
    .unwrap();

    assert_eq!(result.0.len(), 1);
    assert_eq!(result.0[0].title, "Scoped step");
}

/// get_task_steps_http — scope header present with different project ID returns 403.
#[tokio::test]
async fn test_get_task_steps_scope_header_mismatched_project_returns_403() {
    let state = setup_test_state().await;
    let project_id = ProjectId::new();
    let task = Task::new(project_id.clone(), "Scoped task mismatch".to_string());
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    // Scope header present with a DIFFERENT project ID → 403 Forbidden
    let other_project_id = ProjectId::new();
    let result = get_task_steps_http(
        State(state.clone()),
        ProjectScope(Some(vec![other_project_id])),
        Path(task_id.as_str().to_string()),
    )
    .await;

    match result {
        Err(status) => assert_eq!(
            status,
            axum::http::StatusCode::FORBIDDEN,
            "expected 403 when task's project is not in scope"
        ),
        Ok(_) => panic!("expected 403 for out-of-scope project"),
    }
}

/// complete_step with only some steps done keeps IPR entry intact.
#[tokio::test]
async fn test_complete_step_partial_done_keeps_ipr() {
    let state = setup_test_state().await;

    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Partial steps test".to_string());
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    // Create 2 steps
    let step1_resp = add_step_http(
        State(state.clone()),
        Json(AddStepRequest {
            task_id: task_id.as_str().to_string(),
            title: "Step A".to_string(),
            description: None,
            after_step_id: None,
            parent_step_id: None,
            scope_context: None,
        }),
    )
    .await
    .unwrap();
    let step1_id = step1_resp.0.id.clone();

    // Step 2 remains Pending (created but not started)
    let _ = add_step_http(
        State(state.clone()),
        Json(AddStepRequest {
            task_id: task_id.as_str().to_string(),
            title: "Step B".to_string(),
            description: None,
            after_step_id: None,
            parent_step_id: None,
            scope_context: None,
        }),
    )
    .await
    .unwrap();

    // Start only step 1
    let _ = start_step_http(
        State(state.clone()),
        Json(StartStepRequest {
            step_id: step1_id.clone(),
        }),
    )
    .await
    .unwrap();

    // Register IPR
    let (mut child, stdin) = spawn_test_stdin().await;
    let key = InteractiveProcessKey::new("task_execution", task_id.as_str());
    state
        .app_state
        .interactive_process_registry
        .register(key.clone(), stdin)
        .await;

    // Complete step 1 — step 2 is Pending, IPR must remain
    let _ = complete_step_http(
        State(state.clone()),
        Json(CompleteStepRequest {
            step_id: step1_id,
            note: None,
        }),
    )
    .await
    .unwrap();

    assert!(
        state
            .app_state
            .interactive_process_registry
            .has_process(&key)
            .await,
        "IPR must remain when not all steps are done"
    );

    // Clean up: remove IPR so ChildStdin is dropped cleanly
    state
        .app_state
        .interactive_process_registry
        .remove(&key)
        .await;
    let _ = child.kill().await;
}

// ============================================================================
// Validation cache integration tests
// ============================================================================

/// Helper: create a temp git repo with one commit and return (TempDir, commit_sha)
fn create_temp_git_repo() -> (tempfile::TempDir, String) {
    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let repo_path = tmp_dir.path();

    let run_git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed")
    };

    run_git(&["init", "-b", "main"]);
    run_git(&["config", "user.email", "test@test.com"]);
    run_git(&["config", "user.name", "Test"]);
    std::fs::write(repo_path.join("README.md"), "test").unwrap();
    run_git(&["add", "."]);
    run_git(&["commit", "-m", "init"]);

    let sha_output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .expect("git rev-parse failed");
    let sha = String::from_utf8(sha_output.stdout)
        .unwrap()
        .trim()
        .to_string();

    (tmp_dir, sha)
}

#[tokio::test]
async fn test_execution_complete_does_not_cache_agent_reported_tests_passed() {
    let (tmp_dir, _) = create_temp_git_repo();
    let repo_path = tmp_dir.path().to_str().unwrap().to_string();

    let state = setup_test_state().await;

    // Create a task with worktree_path pointing to the git repo
    let project = create_test_project(&state, "Validation Cache Project", &repo_path).await;
    let mut task = Task::new(project.id, "Validation Cache Test Task".to_string());
    mark_no_code_changes(&mut task);
    let task_id = task.id.clone();
    task.worktree_path = Some(repo_path.clone());
    state.app_state.task_repo.create(task).await.unwrap();
    state
        .app_state
        .task_repo
        .persist_status_change(
            &task_id,
            InternalStatus::Ready,
            InternalStatus::Executing,
            "test",
        )
        .await
        .unwrap();

    // Call execution_complete_http with test_result (tests passed)
    let req = ExecutionCompleteRequest {
        summary: Some("All done".to_string()),
        test_result: Some(TestResultInput {
            tests_ran: true,
            tests_passed: true,
            test_summary: Some("42 passed, 0 failed".to_string()),
        }),
    };
    let response = execution_complete_http(
        State(state.clone()),
        Path(task_id.as_str().to_string()),
        Json(req),
    )
    .await
    .unwrap();
    assert!(response.0.success, "execution_complete should succeed");

    let updated_task = state
        .app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    let metadata: serde_json::Value =
        serde_json::from_str(updated_task.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["no_code_changes"], true);

    let context = get_task_context_impl(&state.app_state, &task_id)
        .await
        .expect("get_task_context_impl should succeed");
    assert!(
        context.validation_cache.is_none(),
        "agent-reported successful tests must not become validation cache evidence"
    );
}

#[tokio::test]
async fn test_execution_complete_does_not_cache_agent_reported_no_tests_ran() {
    let (tmp_dir, _) = create_temp_git_repo();
    let repo_path = tmp_dir.path().to_str().unwrap().to_string();

    let state = setup_test_state().await;

    let project = create_test_project(&state, "No Tests Project", &repo_path).await;
    let mut task = Task::new(project.id, "No Tests Task".to_string());
    mark_no_code_changes(&mut task);
    let task_id = task.id.clone();
    task.worktree_path = Some(repo_path.clone());
    state.app_state.task_repo.create(task).await.unwrap();
    state
        .app_state
        .task_repo
        .persist_status_change(
            &task_id,
            InternalStatus::Ready,
            InternalStatus::Executing,
            "test",
        )
        .await
        .unwrap();

    // tests_ran=false — no tests in this task
    let req = ExecutionCompleteRequest {
        summary: None,
        test_result: Some(TestResultInput {
            tests_ran: false,
            tests_passed: false,
            test_summary: None,
        }),
    };
    let response = execution_complete_http(
        State(state.clone()),
        Path(task_id.as_str().to_string()),
        Json(req),
    )
    .await
    .unwrap();
    assert!(response.0.success);

    let context = get_task_context_impl(&state.app_state, &task_id)
        .await
        .expect("get_task_context_impl should succeed");
    assert!(
        context.validation_cache.is_none(),
        "agent-reported no-tests result must not become validation cache evidence"
    );
}

#[tokio::test]
async fn test_execution_complete_without_test_result_leaves_no_cache() {
    let (tmp_dir, _) = create_temp_git_repo();
    let repo_path = tmp_dir.path().to_str().unwrap().to_string();

    let state = setup_test_state().await;

    let project = create_test_project(&state, "No Cache Project", &repo_path).await;
    let mut task = Task::new(project.id, "No Cache Task".to_string());
    mark_no_code_changes(&mut task);
    let task_id = task.id.clone();
    task.worktree_path = Some(repo_path.clone());
    state.app_state.task_repo.create(task).await.unwrap();

    // No test_result — backward-compatible case
    let req = ExecutionCompleteRequest {
        summary: Some("done".to_string()),
        test_result: None,
    };
    let response = execution_complete_http(
        State(state.clone()),
        Path(task_id.as_str().to_string()),
        Json(req),
    )
    .await
    .unwrap();
    assert!(response.0.success);

    // Context should have no validation_cache when no test_result was provided
    let context = get_task_context_impl(&state.app_state, &task_id)
        .await
        .expect("get_task_context_impl should succeed");
    assert!(
        context.validation_cache.is_none(),
        "validation_cache should be None when execution_complete had no test_result"
    );
}

/// execution_complete only accepts the worker signal; stream completion owns the event.
#[tokio::test]
async fn test_execution_complete_does_not_emit_external_event_before_stream_success() {
    let state = setup_test_state().await;

    let project = create_test_project(&state, "Webhook Project", "/tmp/webhook-project").await;
    let project_id = project.id.clone();
    let mut task = Task::new(project_id.clone(), "Webhook emission test".to_string());
    mark_no_code_changes(&mut task);
    let task_id = task.id.clone();
    state.app_state.task_repo.create(task).await.unwrap();

    let req = ExecutionCompleteRequest {
        summary: Some("done".to_string()),
        test_result: None,
    };
    let response = execution_complete_http(
        State(state.clone()),
        Path(task_id.as_str().to_string()),
        Json(req),
    )
    .await
    .unwrap();
    assert!(response.0.success);

    // The accepted tool signal is not final transition authority. Emitting here would
    // falsely report completion before the worker stream settles successfully.
    let events = state
        .app_state
        .external_events_repo
        .get_events_after_cursor(&[project_id.as_str().to_string()], 0, 100)
        .await
        .expect("get_events_after_cursor should succeed");

    assert!(
        events
            .iter()
            .all(|event| event.event_type != "task:execution_completed"),
        "task:execution_completed must wait for authoritative stream success"
    );
}
