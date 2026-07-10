use std::sync::Arc;

use super::execution_plan_control_service::{
    ExecutionPlanControlScope, ExecutionPlanControlService,
};
use crate::application::chat_service::PauseReason;
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    ExecutionPlan, ExecutionPlanHaltMode, ExecutionPlanId, IdeationSession, IdeationSessionId,
    InternalStatus, Project, ProjectId, Task,
};

struct ControlFixture {
    state: AppState,
    project_id: ProjectId,
    session_id: IdeationSessionId,
    current_plan_id: ExecutionPlanId,
    old_plan_id: ExecutionPlanId,
}

async fn setup_control_fixture() -> ControlFixture {
    let state = AppState::new_test();
    let project = Project::new(
        "Plan Controls".to_string(),
        "/tmp/ralphx-plan-controls".to_string(),
    );
    let project = state.project_repo.create(project).await.unwrap();

    let mut session = IdeationSession::new(project.id.clone());
    session.mark_accepted();
    let session = state.ideation_session_repo.create(session).await.unwrap();

    let old_plan = state
        .execution_plan_repo
        .create(ExecutionPlan::new(session.id.clone()))
        .await
        .unwrap();
    state
        .execution_plan_repo
        .mark_superseded(&old_plan.id)
        .await
        .unwrap();
    let current_plan = state
        .execution_plan_repo
        .create(ExecutionPlan::new(session.id.clone()))
        .await
        .unwrap();

    ControlFixture {
        state,
        project_id: project.id,
        session_id: session.id,
        current_plan_id: current_plan.id,
        old_plan_id: old_plan.id,
    }
}

async fn create_plan_task(
    state: &AppState,
    project_id: &ProjectId,
    session_id: &IdeationSessionId,
    execution_plan_id: &ExecutionPlanId,
    status: InternalStatus,
    title: &str,
) -> Task {
    let mut task = Task::new(project_id.clone(), title.to_string());
    task.ideation_session_id = Some(session_id.clone());
    task.execution_plan_id = Some(execution_plan_id.clone());
    task.internal_status = status;
    state.task_repo.create(task).await.unwrap()
}

fn scope(fixture: &ControlFixture) -> ExecutionPlanControlScope {
    ExecutionPlanControlScope {
        project_id: fixture.project_id.clone(),
        session_id: fixture.session_id.clone(),
        execution_plan_id: Some(fixture.current_plan_id.clone()),
    }
}

#[tokio::test]
async fn pause_plan_sets_halt_mode_and_pauses_only_current_plan_active_tasks() {
    let fixture = setup_control_fixture().await;
    let current_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Current plan task",
    )
    .await;
    let stale_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.old_plan_id,
        InternalStatus::Executing,
        "Stale plan task",
    )
    .await;

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let outcome = service.pause_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.execution_plan_id, fixture.current_plan_id);
    assert_eq!(outcome.affected_count, 1);

    let current_plan = fixture
        .state
        .execution_plan_repo
        .get_by_id(&fixture.current_plan_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current_plan.halt_mode, ExecutionPlanHaltMode::Paused);

    let current = fixture
        .state
        .task_repo
        .get_by_id(&current_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.internal_status, InternalStatus::Paused);
    let pause_reason = PauseReason::from_task_metadata(current.metadata.as_deref())
        .expect("pause metadata should be written");
    match pause_reason {
        PauseReason::UserInitiated {
            previous_status,
            scope,
            ..
        } => {
            assert_eq!(previous_status, "executing");
            assert_eq!(scope, "execution_plan");
        }
        PauseReason::ProviderError { .. } => panic!("expected user initiated pause reason"),
    }

    let stale = fixture
        .state
        .task_repo
        .get_by_id(&stale_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stale.internal_status, InternalStatus::Executing);
}

#[tokio::test]
async fn stop_plan_sets_halt_mode_and_stops_only_current_plan_active_tasks() {
    let fixture = setup_control_fixture().await;
    let current_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Current plan execution",
    )
    .await;
    let stale_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.old_plan_id,
        InternalStatus::Executing,
        "Stale plan execution",
    )
    .await;

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let outcome = service.stop_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.execution_plan_id, fixture.current_plan_id);
    assert_eq!(outcome.affected_count, 1);

    let current_plan = fixture
        .state
        .execution_plan_repo
        .get_by_id(&fixture.current_plan_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current_plan.halt_mode, ExecutionPlanHaltMode::Stopped);

    let current = fixture
        .state
        .task_repo
        .get_by_id(&current_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.internal_status, InternalStatus::Stopped);

    let stale = fixture
        .state
        .task_repo
        .get_by_id(&stale_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stale.internal_status, InternalStatus::Executing);
}
