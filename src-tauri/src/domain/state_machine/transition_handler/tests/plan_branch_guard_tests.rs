// Tests for plan branch status guards in on_enter(Executing) and on_enter(ReExecuting).
//
// Guards prevent task execution when the plan branch is Merged or Abandoned.
// Non-plan tasks (no execution_plan_id) bypass the guard.
// Uses execution_plan_id (not session_id) to handle re-accept flows correctly.

use crate::domain::entities::{
    ArtifactId, ExecutionPlanId, IdeationSessionId, InternalStatus, PlanBranch, PlanBranchStatus,
    ProjectId, Task, TaskId,
};
use crate::domain::repositories::{PlanBranchRepository, TaskRepository};
use crate::domain::state_machine::context::{TaskContext, TaskServices};
use crate::domain::state_machine::{State, TaskStateMachine, TransitionHandler};
use crate::error::AppError;
use crate::infrastructure::memory::{MemoryPlanBranchRepository, MemoryTaskRepository};
use std::sync::Arc;

/// Helper: create a machine wired with task_repo + plan_branch_repo for guard testing.
/// Sets execution_plan_id on the task and links the plan branch via execution_plan_id.
async fn setup_guard_test(
    task_id_str: &str,
    exec_plan_id: Option<&str>,
    branch_status: Option<PlanBranchStatus>,
) -> (
    TaskStateMachine,
    Arc<MemoryTaskRepository>,
    Arc<MemoryPlanBranchRepository>,
) {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let ep_id = exec_plan_id.map(|s| ExecutionPlanId::from_string(s.to_string()));

    // Create task with the given execution_plan_id
    let mut task = Task::new(project_id.clone(), "Guard test task".to_string());
    task.id = TaskId::from_string(task_id_str.to_string());
    task.execution_plan_id = ep_id.clone();
    task.internal_status = InternalStatus::Executing;
    task_repo.create(task).await.unwrap();

    // If a branch status is provided, create a plan branch linked by execution_plan_id
    if let (Some(status), Some(ref epid)) = (branch_status, &ep_id) {
        let session_id = IdeationSessionId::from_string(format!("session-{}", epid.as_str()));
        let mut branch = PlanBranch::new(
            ArtifactId::from_string("art-test"),
            session_id,
            project_id.clone(),
            format!("ralphx/test/plan-{}", epid.as_str()),
            "main".to_string(),
        );
        branch.status = status;
        branch.execution_plan_id = Some(epid.clone());
        plan_branch_repo.create(branch).await.unwrap();
    }

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);

    let context = TaskContext::new(task_id_str, "proj-1", services);
    let machine = TaskStateMachine::new(context);

    (machine, task_repo, plan_branch_repo)
}

#[tokio::test]
async fn test_on_enter_executing_blocks_merged_branch() {
    let (mut machine, _, _) =
        setup_guard_test("task-merged", Some("ep-m"), Some(PlanBranchStatus::Merged)).await;

    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::Executing).await;

    assert!(result.is_err(), "Should block execution on merged branch");
    let err = result.unwrap_err();
    assert!(
        matches!(err, AppError::ExecutionBlocked(_)),
        "Error should be ExecutionBlocked, got: {err}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("merged"),
        "Error message should mention merged status: {msg}"
    );
}

#[tokio::test]
async fn test_on_enter_executing_blocks_abandoned_branch() {
    let (mut machine, _, _) = setup_guard_test(
        "task-aband",
        Some("ep-a"),
        Some(PlanBranchStatus::Abandoned),
    )
    .await;

    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::Executing).await;

    assert!(
        result.is_err(),
        "Should block execution on abandoned branch"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, AppError::ExecutionBlocked(_)),
        "Error should be ExecutionBlocked, got: {err}"
    );
}

#[tokio::test]
async fn test_on_enter_executing_allows_active_branch() {
    let (mut machine, _, _) =
        setup_guard_test("task-active", Some("ep-ok"), Some(PlanBranchStatus::Active)).await;

    let handler = TransitionHandler::new(&mut machine);
    // on_enter(Executing) will proceed past the guard but may fail later
    // (no git repo, etc.) — we only care that it doesn't return ExecutionBlocked
    let result = handler.on_enter(&State::Executing).await;

    match result {
        Ok(_) => {} // Passed guard (and possibly no-op'd the rest)
        Err(AppError::ExecutionBlocked(msg)) if msg.contains("inactive branch") => {
            panic!("Active branch should not trigger ExecutionBlocked: {msg}");
        }
        Err(_) => {} // Other errors (git, etc.) are expected without a real repo
    }
}

#[tokio::test]
async fn test_on_enter_executing_allows_non_plan_task() {
    // Task with no execution_plan_id bypasses the guard
    let (mut machine, _, _) = setup_guard_test("task-noplan", None, None).await;

    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::Executing).await;

    match result {
        Ok(_) => {}
        Err(AppError::ExecutionBlocked(msg)) if msg.contains("inactive branch") => {
            panic!("Non-plan task should not trigger branch guard: {msg}");
        }
        Err(_) => {} // Other errors (git, etc.) are expected
    }
}

#[tokio::test]
async fn test_on_enter_reexecuting_blocks_merged_branch() {
    let (mut machine, _, _) =
        setup_guard_test("task-re-m", Some("ep-rem"), Some(PlanBranchStatus::Merged)).await;

    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::ReExecuting).await;

    assert!(
        result.is_err(),
        "Should block re-execution on merged branch"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, AppError::ExecutionBlocked(_)),
        "Error should be ExecutionBlocked, got: {err}"
    );
}

#[tokio::test]
async fn test_on_enter_reexecuting_allows_active_branch() {
    let (mut machine, _, _) = setup_guard_test(
        "task-re-ok",
        Some("ep-reok"),
        Some(PlanBranchStatus::Active),
    )
    .await;

    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::ReExecuting).await;

    match result {
        Ok(_) => {}
        Err(AppError::ExecutionBlocked(msg)) if msg.contains("inactive branch") => {
            panic!("Active branch should not trigger ExecutionBlocked in ReExecuting: {msg}");
        }
        Err(_) => {} // Other errors are expected without full environment
    }
}
