// Integration tests for step reset on execution re-entry.
//
// Covers:
//   on_enter(Executing): steps with terminal statuses are reset to Pending, step:updated emitted
//   on_enter(ReExecuting): same reset behavior on revision entry path
//
// Test strategy:
//   - Use MemoryTaskStepRepository wired into TaskServices
//   - Use MemoryTaskRepository + MemoryPlanBranchRepository for plan-branch guard
//   - No task_repo/project_repo → skips worktree setup (guarded by if let Some)
//   - Assert steps are Pending after on_enter; assert step:updated event emitted

use super::helpers::*;
use crate::domain::entities::{
    ArtifactId, ExecutionPlanId, IdeationSessionId, InternalStatus, PlanBranch, PlanBranchStatus,
    ProjectId, Task, TaskId, TaskStep, TaskStepStatus,
};
use crate::domain::repositories::{PlanBranchRepository, TaskRepository, TaskStepRepository};
use crate::domain::state_machine::context::TaskContext;
use crate::domain::state_machine::mocks::MockTaskScheduler;
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::{State, TaskStateMachine, TransitionHandler};
use crate::infrastructure::memory::{MemoryPlanBranchRepository, MemoryTaskStepRepository};

// ──────────────────────────────────────────────────────────────────────────────
// Shared setup helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Build a machine wired with step_repo + task_repo + plan_branch_repo.
/// Sets up an Active plan branch so the guard passes.
/// Returns the machine plus references for assertions.
async fn setup_for_step_reset(
    task_id_str: &str,
    exec_plan_id: &str,
    step_statuses: Vec<TaskStepStatus>,
) -> (
    TaskStateMachine,
    Arc<MemoryTaskStepRepository>,
    Arc<MockEventEmitter>,
) {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let step_repo = Arc::new(MemoryTaskStepRepository::new());
    let event_emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-step-reset".to_string());
    let ep_id = ExecutionPlanId::from_string(exec_plan_id.to_string());

    // Create task with execution_plan_id so guard consults the repo
    let mut task = Task::new(project_id.clone(), "Step reset test task".to_string());
    task.id = TaskId::from_string(task_id_str.to_string());
    task.execution_plan_id = Some(ep_id.clone());
    task.internal_status = InternalStatus::Executing;
    task_repo.create(task).await.unwrap();

    // Create an Active plan branch linked by execution_plan_id
    let session_id = IdeationSessionId::from_string(format!("sess-{}", exec_plan_id));
    let mut branch = PlanBranch::new(
        ArtifactId::from_string("art-step-reset"),
        session_id,
        project_id,
        format!("ralphx/test/plan-{}", exec_plan_id),
        "main".to_string(),
    );
    branch.status = PlanBranchStatus::Active;
    branch.execution_plan_id = Some(ep_id);
    plan_branch_repo.create(branch).await.unwrap();

    // Create steps with the given statuses
    let task_id_typed = TaskId::from_string(task_id_str.to_string());
    for (i, status) in step_statuses.into_iter().enumerate() {
        let mut step = TaskStep::new(
            task_id_typed.clone(),
            format!("Step {}", i),
            i as i32,
            "proposal".to_string(),
        );
        step.status = status;
        step_repo.create(step).await.unwrap();
    }

    let services = TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::clone(&event_emitter) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    )
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>)
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
    .with_step_repo(Arc::clone(&step_repo) as Arc<dyn TaskStepRepository>);

    let context = TaskContext::new(task_id_str, "proj-step-reset", services);
    let machine = TaskStateMachine::new(context);

    (machine, step_repo, event_emitter)
}

// ──────────────────────────────────────────────────────────────────────────────
// on_enter(Executing) tests
// ──────────────────────────────────────────────────────────────────────────────

/// on_enter(Executing) resets Completed steps to Pending and emits step:updated.
#[tokio::test]
async fn on_enter_executing_resets_completed_steps_to_pending() {
    let task_id = "task-exec-reset-1";
    let (mut machine, step_repo, event_emitter) = setup_for_step_reset(
        task_id,
        "ep-exec-1",
        vec![
            TaskStepStatus::Completed,
            TaskStepStatus::Completed,
            TaskStepStatus::Completed,
        ],
    )
    .await;

    let handler = TransitionHandler::new(&mut machine);
    // Result may be Ok or Err (git/worktree errors expected in test env) — we only care about side effects
    let _ = handler.on_enter(&State::Executing).await;

    // All steps should be Pending
    let task_id_typed = TaskId::from_string(task_id.to_string());
    let steps = step_repo.get_by_task(&task_id_typed).await.unwrap();
    assert_eq!(steps.len(), 3);
    for step in &steps {
        assert_eq!(
            step.status,
            TaskStepStatus::Pending,
            "Step '{}' should be Pending after on_enter(Executing)",
            step.title
        );
    }

    // step:updated event should have been emitted with the task_id
    assert!(
        event_emitter.has_event("step:updated"),
        "step:updated event should be emitted when steps are reset"
    );
}

/// on_enter(Executing) handles mixed statuses: Completed, InProgress, Failed, Pending.
#[tokio::test]
async fn on_enter_executing_resets_mixed_statuses() {
    let task_id = "task-exec-reset-2";
    let (mut machine, step_repo, event_emitter) = setup_for_step_reset(
        task_id,
        "ep-exec-2",
        vec![
            TaskStepStatus::Pending,
            TaskStepStatus::Completed,
            TaskStepStatus::InProgress,
            TaskStepStatus::Failed,
        ],
    )
    .await;

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Executing).await;

    let task_id_typed = TaskId::from_string(task_id.to_string());
    let steps = step_repo.get_by_task(&task_id_typed).await.unwrap();
    for step in &steps {
        assert_eq!(
            step.status,
            TaskStepStatus::Pending,
            "All steps should be Pending after reset"
        );
    }

    assert!(event_emitter.has_event("step:updated"));
}

/// on_enter(Executing) is a no-op when all steps are already Pending (first execution).
/// No step:updated event emitted.
#[tokio::test]
async fn on_enter_executing_noop_when_steps_already_pending() {
    let task_id = "task-exec-noop-1";
    let (mut machine, _, event_emitter) = setup_for_step_reset(
        task_id,
        "ep-exec-noop",
        vec![TaskStepStatus::Pending, TaskStepStatus::Pending],
    )
    .await;

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Executing).await;

    // No step:updated event when count == 0
    assert!(
        !event_emitter.has_event("step:updated"),
        "step:updated should NOT be emitted when all steps are already Pending"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// on_enter(ReExecuting) tests
// ──────────────────────────────────────────────────────────────────────────────

/// on_enter(ReExecuting) resets Completed steps to Pending and emits step:updated.
#[tokio::test]
async fn on_enter_reexecuting_resets_completed_steps_to_pending() {
    let task_id = "task-reexec-reset-1";
    let (mut machine, step_repo, event_emitter) = setup_for_step_reset(
        task_id,
        "ep-reexec-1",
        vec![
            TaskStepStatus::Completed,
            TaskStepStatus::Skipped,
            TaskStepStatus::Cancelled,
        ],
    )
    .await;

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::ReExecuting).await;

    let task_id_typed = TaskId::from_string(task_id.to_string());
    let steps = step_repo.get_by_task(&task_id_typed).await.unwrap();
    assert_eq!(steps.len(), 3);
    for step in &steps {
        assert_eq!(
            step.status,
            TaskStepStatus::Pending,
            "Step '{}' should be Pending after on_enter(ReExecuting)",
            step.title
        );
    }

    assert!(
        event_emitter.has_event("step:updated"),
        "step:updated event should be emitted when steps are reset"
    );
}

/// on_enter(ReExecuting) is a no-op when all steps are already Pending.
#[tokio::test]
async fn on_enter_reexecuting_noop_when_steps_already_pending() {
    let task_id = "task-reexec-noop-1";
    let (mut machine, _, event_emitter) =
        setup_for_step_reset(task_id, "ep-reexec-noop", vec![TaskStepStatus::Pending]).await;

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::ReExecuting).await;

    assert!(
        !event_emitter.has_event("step:updated"),
        "step:updated should NOT be emitted when all steps are already Pending"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// preserve_steps flag tests
// ──────────────────────────────────────────────────────────────────────────────

/// Build a machine with task_repo (holding a task with the given metadata) + step_repo.
/// Returns (machine, task_repo, step_repo, event_emitter) for full assertion access.
async fn setup_with_metadata(
    task_id_str: &str,
    exec_plan_id: &str,
    step_statuses: Vec<TaskStepStatus>,
    metadata: Option<&str>,
) -> (
    TaskStateMachine,
    Arc<MemoryTaskRepository>,
    Arc<MemoryTaskStepRepository>,
    Arc<MockEventEmitter>,
) {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let step_repo = Arc::new(MemoryTaskStepRepository::new());
    let event_emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-preserve".to_string());
    let ep_id = ExecutionPlanId::from_string(exec_plan_id.to_string());

    let mut task = Task::new(project_id.clone(), "Preserve steps test task".to_string());
    task.id = TaskId::from_string(task_id_str.to_string());
    task.execution_plan_id = Some(ep_id.clone());
    task.internal_status = InternalStatus::Executing;
    task.metadata = metadata.map(|s| s.to_string());
    task_repo.create(task).await.unwrap();

    // Active plan branch so the guard passes
    let session_id = IdeationSessionId::from_string(format!("sess-{}", exec_plan_id));
    let mut branch = PlanBranch::new(
        ArtifactId::from_string("art-preserve"),
        session_id,
        project_id,
        format!("ralphx/test/plan-{}", exec_plan_id),
        "main".to_string(),
    );
    branch.status = PlanBranchStatus::Active;
    branch.execution_plan_id = Some(ep_id);
    plan_branch_repo.create(branch).await.unwrap();

    let task_id_typed = TaskId::from_string(task_id_str.to_string());
    for (i, status) in step_statuses.into_iter().enumerate() {
        let mut step = TaskStep::new(
            task_id_typed.clone(),
            format!("Step {}", i),
            i as i32,
            "proposal".to_string(),
        );
        step.status = status;
        step_repo.create(step).await.unwrap();
    }

    let services = TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::clone(&event_emitter) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    )
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>)
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
    .with_step_repo(Arc::clone(&step_repo) as Arc<dyn TaskStepRepository>);

    let context = TaskContext::new(task_id_str, "proj-preserve", services);
    let machine = TaskStateMachine::new(context);

    (machine, task_repo, step_repo, event_emitter)
}

/// preserve_steps=true skips reset: steps retain their original statuses and step:updated is emitted.
#[tokio::test]
async fn preserve_steps_flag_skips_step_reset() {
    let task_id = "task-preserve-1";
    let metadata = r#"{"preserve_steps": true}"#;
    let (mut machine, _, step_repo, event_emitter) = setup_with_metadata(
        task_id,
        "ep-preserve-1",
        vec![
            TaskStepStatus::Completed,
            TaskStepStatus::Failed,
            TaskStepStatus::InProgress,
            TaskStepStatus::Pending,
        ],
        Some(metadata),
    )
    .await;

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Executing).await;

    let task_id_typed = TaskId::from_string(task_id.to_string());
    let steps = step_repo.get_by_task(&task_id_typed).await.unwrap();
    assert_eq!(steps.len(), 4);

    // Steps should retain their original statuses (not all reset to Pending)
    let statuses: Vec<_> = steps.iter().map(|s| s.status.clone()).collect();
    assert!(
        statuses.contains(&TaskStepStatus::Completed),
        "Completed step should be preserved"
    );
    assert!(
        statuses.contains(&TaskStepStatus::Failed),
        "Failed step should be preserved"
    );
    assert!(
        statuses.contains(&TaskStepStatus::InProgress),
        "InProgress step should be preserved"
    );

    // step:updated should be emitted for UI refresh
    assert!(
        event_emitter.has_event("step:updated"),
        "step:updated event should be emitted when steps are preserved"
    );
}

/// preserve_steps flag is cleared (one-shot) after on_enter reads it.
#[tokio::test]
async fn preserve_steps_flag_cleared_after_use() {
    let task_id = "task-preserve-clear-1";
    let metadata = r#"{"preserve_steps": true, "other_key": "kept"}"#;
    let (mut machine, task_repo, _, _) = setup_with_metadata(
        task_id,
        "ep-preserve-clear-1",
        vec![TaskStepStatus::Completed],
        Some(metadata),
    )
    .await;

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Executing).await;

    // The task's metadata should no longer contain preserve_steps
    let task_id_typed = TaskId::from_string(task_id.to_string());
    let task = task_repo.get_by_id(&task_id_typed).await.unwrap().unwrap();
    let meta_str = task.metadata.as_deref().unwrap_or("{}");
    let meta: serde_json::Value = serde_json::from_str(meta_str).unwrap();
    assert!(
        meta.get("preserve_steps").is_none(),
        "preserve_steps flag must be cleared after one-shot consumption, got: {}",
        meta_str
    );
    // Other metadata keys must be preserved
    assert_eq!(
        meta.get("other_key").and_then(|v| v.as_str()),
        Some("kept"),
        "Unrelated metadata keys must be preserved when clearing preserve_steps"
    );
}

/// Auto-recovery (no preserve_steps flag) continues to reset all steps to Pending.
#[tokio::test]
async fn auto_recovery_without_flag_resets_steps() {
    let task_id = "task-auto-recover-1";
    let (mut machine, _, step_repo, _) = setup_with_metadata(
        task_id,
        "ep-auto-recover-1",
        vec![
            TaskStepStatus::Completed,
            TaskStepStatus::Failed,
            TaskStepStatus::InProgress,
        ],
        None, // no metadata → no preserve_steps flag
    )
    .await;

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Executing).await;

    let task_id_typed = TaskId::from_string(task_id.to_string());
    let steps = step_repo.get_by_task(&task_id_typed).await.unwrap();
    for step in &steps {
        assert_eq!(
            step.status,
            TaskStepStatus::Pending,
            "Step '{}' should be reset to Pending when preserve_steps flag is absent",
            step.title
        );
    }
}

/// When task is not found in the repo (Ok(None)), falls through to reset all steps to Pending.
#[tokio::test]
async fn task_not_found_fallback_resets_steps() {
    let task_id = "task-fallback-1";
    // Use a different task_id in the machine context than what's stored — task won't be found
    let missing_task_id = "task-fallback-missing";
    let metadata = r#"{"preserve_steps": true}"#;

    // Insert task with task_id but wire the machine with missing_task_id
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let step_repo = Arc::new(MemoryTaskStepRepository::new());
    let event_emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-fallback".to_string());
    let ep_id = ExecutionPlanId::from_string("ep-fallback-1".to_string());

    // Insert task using task_id (machine will look for missing_task_id → Ok(None))
    let mut stored_task = Task::new(project_id.clone(), "Stored task".to_string());
    stored_task.id = TaskId::from_string(task_id.to_string());
    stored_task.metadata = Some(metadata.to_string());
    stored_task.execution_plan_id = Some(ep_id.clone());
    stored_task.internal_status = InternalStatus::Executing;
    task_repo.create(stored_task).await.unwrap();

    // Create an Active plan branch for missing_task_id's project/ep
    let session_id = IdeationSessionId::from_string("sess-fallback-1".to_string());
    let mut branch = PlanBranch::new(
        ArtifactId::from_string("art-fallback"),
        session_id,
        project_id,
        "ralphx/test/plan-fallback".to_string(),
        "main".to_string(),
    );
    branch.status = PlanBranchStatus::Active;
    branch.execution_plan_id = Some(ep_id);
    plan_branch_repo.create(branch).await.unwrap();

    // Steps belong to missing_task_id (machine's context task_id)
    let missing_tid = TaskId::from_string(missing_task_id.to_string());
    for (i, status) in [TaskStepStatus::Completed, TaskStepStatus::Failed]
        .into_iter()
        .enumerate()
    {
        let mut step = TaskStep::new(
            missing_tid.clone(),
            format!("Step {}", i),
            i as i32,
            "proposal".to_string(),
        );
        step.status = status;
        step_repo.create(step).await.unwrap();
    }

    let services = TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::clone(&event_emitter) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    )
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>)
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
    .with_step_repo(Arc::clone(&step_repo) as Arc<dyn TaskStepRepository>);

    // Machine context uses missing_task_id → task_repo.get_by_id returns Ok(None) → fallback
    let context = TaskContext::new(missing_task_id, "proj-fallback", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Executing).await;

    // Steps should be reset to Pending (safe fallback)
    let steps = step_repo.get_by_task(&missing_tid).await.unwrap();
    for step in &steps {
        assert_eq!(
            step.status,
            TaskStepStatus::Pending,
            "Step '{}' should be reset to Pending when task not found in repo (safe fallback)",
            step.title
        );
    }
}
