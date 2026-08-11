use std::sync::Arc;

use super::execution_plan_controls::{execute_execution_plan_control, ExecutionPlanControlAction};
use super::types::ExecutionPlanControlInput;
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    ExecutionPlan, ExecutionPlanHaltMode, ExecutionPlanId, IdeationSession, IdeationSessionId,
    InternalStatus, Project, ProjectId, Task, TaskId,
};

struct CommandFixture {
    state: AppState,
    project_id: ProjectId,
    session_id: IdeationSessionId,
    current_plan_id: ExecutionPlanId,
    old_plan_id: ExecutionPlanId,
}

async fn setup_command_fixture() -> CommandFixture {
    let state = AppState::new_test();
    let project = Project::new(
        "Plan Command Controls".to_string(),
        "/tmp/ralphx-plan-command-controls".to_string(),
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

    CommandFixture {
        state,
        project_id: project.id,
        session_id: session.id,
        current_plan_id: current_plan.id,
        old_plan_id: old_plan.id,
    }
}

async fn create_plan_task(fixture: &CommandFixture, status: InternalStatus, title: &str) -> Task {
    let mut task = Task::new(fixture.project_id.clone(), title.to_string());
    task.ideation_session_id = Some(fixture.session_id.clone());
    task.execution_plan_id = Some(fixture.current_plan_id.clone());
    task.internal_status = status;
    fixture.state.task_repo.create(task).await.unwrap()
}

async fn stored_task(fixture: &CommandFixture, task_id: &TaskId) -> Task {
    fixture
        .state
        .task_repo
        .get_by_id(task_id)
        .await
        .unwrap()
        .unwrap()
}

async fn current_plan_halt_mode(fixture: &CommandFixture) -> ExecutionPlanHaltMode {
    fixture
        .state
        .execution_plan_repo
        .get_by_id(&fixture.current_plan_id)
        .await
        .unwrap()
        .unwrap()
        .halt_mode
}

fn control_input(
    fixture: &CommandFixture,
    execution_plan_id: Option<&ExecutionPlanId>,
) -> ExecutionPlanControlInput {
    ExecutionPlanControlInput {
        project_id: fixture.project_id.as_str().to_string(),
        session_id: fixture.session_id.as_str().to_string(),
        execution_plan_id: execution_plan_id.map(|id| id.as_str().to_string()),
    }
}

async fn execute(
    fixture: &CommandFixture,
    input: ExecutionPlanControlInput,
    action: ExecutionPlanControlAction,
) -> Result<super::types::ExecutionPlanControlResponse, String> {
    execute_execution_plan_control(
        &input,
        &fixture.state,
        Arc::new(ExecutionState::new()),
        None,
        action,
    )
    .await
}

#[tokio::test]
async fn pause_command_uses_current_plan_when_input_omits_plan_id() {
    let fixture = setup_command_fixture().await;
    let task = create_plan_task(&fixture, InternalStatus::Executing, "Pause command task").await;

    let response = execute(
        &fixture,
        control_input(&fixture, None),
        ExecutionPlanControlAction::Pause,
    )
    .await
    .unwrap();

    assert_eq!(response.execution_plan_id, fixture.current_plan_id.as_str());
    assert_eq!(response.affected_count, 1);
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Paused
    );
    assert_eq!(
        stored_task(&fixture, &task.id).await.internal_status,
        InternalStatus::Paused
    );
}

#[tokio::test]
async fn resume_command_returns_zero_when_paused_task_has_no_restore_status() {
    let fixture = setup_command_fixture().await;
    fixture
        .state
        .execution_plan_repo
        .set_halt_mode(&fixture.current_plan_id, ExecutionPlanHaltMode::Paused)
        .await
        .unwrap();
    let task = create_plan_task(&fixture, InternalStatus::Paused, "Resume command task").await;

    let response = execute(
        &fixture,
        control_input(&fixture, Some(&fixture.current_plan_id)),
        ExecutionPlanControlAction::Resume,
    )
    .await
    .unwrap();

    assert_eq!(response.execution_plan_id, fixture.current_plan_id.as_str());
    assert_eq!(response.affected_count, 0);
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Running
    );
    assert_eq!(
        stored_task(&fixture, &task.id).await.internal_status,
        InternalStatus::Paused
    );
}

#[tokio::test]
async fn stop_command_stops_current_plan_active_task() {
    let fixture = setup_command_fixture().await;
    let task = create_plan_task(&fixture, InternalStatus::Merging, "Stop command task").await;

    let response = execute(
        &fixture,
        control_input(&fixture, Some(&fixture.current_plan_id)),
        ExecutionPlanControlAction::Stop,
    )
    .await
    .unwrap();

    assert_eq!(response.execution_plan_id, fixture.current_plan_id.as_str());
    assert_eq!(response.affected_count, 1);
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Stopped
    );
    assert_eq!(
        stored_task(&fixture, &task.id).await.internal_status,
        InternalStatus::Stopped
    );
}

#[tokio::test]
async fn command_rejects_stale_explicit_plan_id() {
    let fixture = setup_command_fixture().await;
    let task = create_plan_task(&fixture, InternalStatus::Executing, "Stale command task").await;

    let error = execute(
        &fixture,
        control_input(&fixture, Some(&fixture.old_plan_id)),
        ExecutionPlanControlAction::Pause,
    )
    .await
    .expect_err("stale explicit execution plan id must fail");

    assert!(
        error.contains("active execution plan"),
        "unexpected stale plan error: {error}"
    );
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Running
    );
    assert_eq!(
        stored_task(&fixture, &task.id).await.internal_status,
        InternalStatus::Executing
    );
}
