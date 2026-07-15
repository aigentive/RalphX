use super::*;
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{ExecutionPlan, ExecutionPlanHaltMode, ExecutionPlanId, Project};
use crate::domain::repositories::ExecutionPlanRepository;
use crate::error::{AppError, AppResult};
use async_trait::async_trait;

fn scheduler_for_state(state: &AppState) -> TaskSchedulerService {
    TaskSchedulerService::new(
        Arc::new(ExecutionState::new()),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.task_repo),
        Arc::clone(&state.task_dependency_repo),
        Arc::clone(&state.artifact_repo),
        Arc::clone(&state.chat_message_repo),
        Arc::clone(&state.chat_attachment_repo),
        Arc::clone(&state.chat_conversation_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::clone(&state.ideation_session_repo),
        Arc::clone(&state.activity_event_repo),
        Arc::clone(&state.message_queue),
        Arc::clone(&state.running_agent_registry),
        Arc::clone(&state.memory_event_repo),
        None,
    )
    .with_execution_plan_repo(Arc::clone(&state.execution_plan_repo))
}

struct FailingExecutionPlanRepository;

#[async_trait]
impl ExecutionPlanRepository for FailingExecutionPlanRepository {
    async fn create(&self, plan: ExecutionPlan) -> AppResult<ExecutionPlan> {
        Ok(plan)
    }

    async fn get_by_id(&self, _id: &ExecutionPlanId) -> AppResult<Option<ExecutionPlan>> {
        Err(AppError::Database("halt lookup failed".to_string()))
    }

    async fn get_by_session(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<Vec<ExecutionPlan>> {
        Ok(Vec::new())
    }

    async fn get_active_for_session(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<Option<ExecutionPlan>> {
        Ok(None)
    }

    async fn mark_superseded(&self, _id: &ExecutionPlanId) -> AppResult<()> {
        Ok(())
    }

    async fn set_halt_mode(
        &self,
        _id: &ExecutionPlanId,
        _halt_mode: ExecutionPlanHaltMode,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _id: &ExecutionPlanId) -> AppResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn scheduler_reblocks_ready_task_with_active_dependency() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let mut task = Task::new(project_id.clone(), "Ready dependent".to_string());
    task.internal_status = InternalStatus::Ready;
    let task = state.task_repo.create(task).await.unwrap();

    let mut blocker = Task::new(project_id, "Executing blocker".to_string());
    blocker.internal_status = InternalStatus::Executing;
    let blocker = state.task_repo.create(blocker).await.unwrap();

    state
        .task_dependency_repo
        .add_dependency(&task.id, &blocker.id)
        .await
        .unwrap();

    let scheduler = scheduler_for_state(&state);

    assert!(
        scheduler.has_unsatisfied_dependencies(&task).await,
        "executing dependency should still block scheduler admission"
    );

    scheduler.reblock_task(&task).await;

    let updated = state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(updated.internal_status, InternalStatus::Blocked);
    assert!(
        updated
            .blocked_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("Executing blocker")),
        "reblock reason should include the active blocker title"
    );
}

#[tokio::test]
async fn execution_plan_halt_guard_allows_running_plan() {
    let state = AppState::new_test();
    let session_id = IdeationSessionId::from_string("session-running-plan");
    let plan = state
        .execution_plan_repo
        .create(ExecutionPlan::new(session_id))
        .await
        .unwrap();

    let mut task = Task::new(ProjectId::new(), "Ready plan task".to_string());
    task.execution_plan_id = Some(plan.id.clone());

    let scheduler = scheduler_for_state(&state);

    assert!(
        !scheduler.is_execution_plan_halted(&task).await,
        "running execution plan should not block scheduler admission"
    );
}

#[tokio::test]
async fn execution_plan_halt_guard_skips_paused_plan() {
    let state = AppState::new_test();
    let session_id = IdeationSessionId::from_string("session-paused-plan");
    let plan = state
        .execution_plan_repo
        .create(ExecutionPlan::new(session_id))
        .await
        .unwrap();
    state
        .execution_plan_repo
        .set_halt_mode(&plan.id, ExecutionPlanHaltMode::Paused)
        .await
        .unwrap();

    let mut task = Task::new(ProjectId::new(), "Ready plan task".to_string());
    task.execution_plan_id = Some(plan.id.clone());

    let scheduler = scheduler_for_state(&state);

    assert!(
        scheduler.is_execution_plan_halted(&task).await,
        "paused execution plan should block scheduler admission"
    );
}

#[tokio::test]
async fn execution_plan_halt_guard_skips_missing_plan() {
    let state = AppState::new_test();
    let mut task = Task::new(ProjectId::new(), "Ready plan task".to_string());
    task.execution_plan_id = Some(ExecutionPlanId::from_string("missing-plan"));

    let scheduler = scheduler_for_state(&state);

    assert!(
        scheduler.is_execution_plan_halted(&task).await,
        "missing execution plan should fail closed"
    );
}

#[tokio::test]
async fn execution_plan_halt_guard_skips_on_repo_error() {
    let state = AppState::new_test();
    let mut task = Task::new(ProjectId::new(), "Ready plan task".to_string());
    task.execution_plan_id = Some(ExecutionPlanId::from_string("error-plan"));

    let scheduler = scheduler_for_state(&state)
        .with_execution_plan_repo(Arc::new(FailingExecutionPlanRepository));

    assert!(
        scheduler.is_execution_plan_halted(&task).await,
        "halt lookup errors should fail closed"
    );
}

#[tokio::test]
async fn scheduler_execution_plan_scope_admits_only_matching_ready_task() {
    let state = AppState::new_test();
    let project = Project::new(
        "Scoped scheduler".to_string(),
        "/tmp/scoped-scheduler".to_string(),
    );
    let project = state.project_repo.create(project).await.unwrap();

    let plan_a = state
        .execution_plan_repo
        .create(ExecutionPlan::new(IdeationSessionId::from_string(
            "session-plan-a",
        )))
        .await
        .unwrap();
    let plan_b = state
        .execution_plan_repo
        .create(ExecutionPlan::new(IdeationSessionId::from_string(
            "session-plan-b",
        )))
        .await
        .unwrap();

    let mut task_a = Task::new(project.id.clone(), "Plan A ready".to_string());
    task_a.internal_status = InternalStatus::Ready;
    task_a.execution_plan_id = Some(plan_a.id.clone());
    state.task_repo.create(task_a).await.unwrap();

    let mut task_b = Task::new(project.id.clone(), "Plan B ready".to_string());
    task_b.internal_status = InternalStatus::Ready;
    task_b.execution_plan_id = Some(plan_b.id.clone());
    let task_b = state.task_repo.create(task_b).await.unwrap();

    let scheduler = scheduler_for_state(&state);
    scheduler
        .set_active_execution_plan(Some(plan_b.id.clone()))
        .await;

    let selected = scheduler
        .find_oldest_schedulable_task_for_test()
        .await
        .expect("plan B task should be schedulable");

    assert_eq!(selected.id, task_b.id);
}
