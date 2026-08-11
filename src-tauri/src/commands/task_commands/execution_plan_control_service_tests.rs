use std::collections::HashMap;
use std::sync::Arc;

use super::execution_plan_control_service::{
    ExecutionPlanControlScope, ExecutionPlanControlService,
};
use crate::application::chat_service::PauseReason;
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    ExecutionPlan, ExecutionPlanHaltMode, ExecutionPlanId, IdeationSession, IdeationSessionId,
    InternalStatus, Project, ProjectId, Task, TaskId,
};
use crate::domain::execution::ExecutionSettings;
use crate::domain::repositories::{StateHistoryMetadata, StatusTransition, TaskRepository};
use crate::error::{AppError, AppResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

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

async fn create_paused_plan_task_with_previous_status(
    state: &AppState,
    project_id: &ProjectId,
    session_id: &IdeationSessionId,
    execution_plan_id: &ExecutionPlanId,
    previous_status: InternalStatus,
    title: &str,
) -> Task {
    let mut task = Task::new(project_id.clone(), title.to_string());
    task.ideation_session_id = Some(session_id.clone());
    task.execution_plan_id = Some(execution_plan_id.clone());
    task.internal_status = InternalStatus::Paused;
    let pause_reason = PauseReason::UserInitiated {
        previous_status: previous_status.to_string(),
        paused_at: chrono::Utc::now().to_rfc3339(),
        scope: "execution_plan".to_string(),
    };
    task.metadata = Some(pause_reason.write_to_task_metadata(None));
    state.task_repo.create(task).await.unwrap()
}

fn scope(fixture: &ControlFixture) -> ExecutionPlanControlScope {
    ExecutionPlanControlScope {
        project_id: fixture.project_id.clone(),
        session_id: fixture.session_id.clone(),
        execution_plan_id: Some(fixture.current_plan_id.clone()),
    }
}

async fn plan_halt_mode(state: &AppState, plan_id: &ExecutionPlanId) -> ExecutionPlanHaltMode {
    state
        .execution_plan_repo
        .get_by_id(plan_id)
        .await
        .unwrap()
        .unwrap()
        .halt_mode
}

async fn current_plan_halt_mode(fixture: &ControlFixture) -> ExecutionPlanHaltMode {
    plan_halt_mode(&fixture.state, &fixture.current_plan_id).await
}

async fn stored_task(state: &AppState, task_id: &TaskId) -> Task {
    state.task_repo.get_by_id(task_id).await.unwrap().unwrap()
}

fn fail_transition_for(fixture: &mut ControlFixture, task_id: TaskId) {
    let inner = Arc::clone(&fixture.state.task_repo);
    fixture.state.task_repo = Arc::new(FailingTaskRepository {
        inner,
        transition_task_id: Some(task_id),
        fail_plan_task_query: false,
        fail_status_history: false,
    });
}

fn fail_plan_task_query(fixture: &mut ControlFixture) {
    let inner = Arc::clone(&fixture.state.task_repo);
    fixture.state.task_repo = Arc::new(FailingTaskRepository {
        inner,
        transition_task_id: None,
        fail_plan_task_query: true,
        fail_status_history: false,
    });
}

fn fail_status_history_for_restore(fixture: &mut ControlFixture) {
    let inner = Arc::clone(&fixture.state.task_repo);
    fixture.state.task_repo = Arc::new(FailingTaskRepository {
        inner,
        transition_task_id: None,
        fail_plan_task_query: false,
        fail_status_history: true,
    });
}

#[tokio::test]
async fn pause_plan_with_only_ready_tasks_halts_plan_without_affecting_tasks() {
    let fixture = setup_control_fixture().await;
    let ready_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Ready,
        "Queued current plan task",
    )
    .await;

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let outcome = service.pause_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.execution_plan_id, fixture.current_plan_id);
    assert_eq!(outcome.affected_count, 0);
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Paused
    );
    assert_eq!(
        stored_task(&fixture.state, &ready_task.id)
            .await
            .internal_status,
        InternalStatus::Ready
    );
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

    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Paused
    );

    let current = stored_task(&fixture.state, &current_task.id).await;
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

    let stale = stored_task(&fixture.state, &stale_task.id).await;
    assert_eq!(stale.internal_status, InternalStatus::Executing);
}

#[tokio::test]
async fn stop_plan_with_only_ready_tasks_halts_plan_without_affecting_tasks() {
    let fixture = setup_control_fixture().await;
    let ready_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Ready,
        "Queued current plan stop task",
    )
    .await;

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let outcome = service.stop_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.execution_plan_id, fixture.current_plan_id);
    assert_eq!(outcome.affected_count, 0);
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Stopped
    );
    assert_eq!(
        stored_task(&fixture.state, &ready_task.id)
            .await
            .internal_status,
        InternalStatus::Ready
    );
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

    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Stopped
    );

    let current = stored_task(&fixture.state, &current_task.id).await;
    assert_eq!(current.internal_status, InternalStatus::Stopped);

    let stale = stored_task(&fixture.state, &stale_task.id).await;
    assert_eq!(stale.internal_status, InternalStatus::Executing);
}

#[tokio::test]
async fn resume_plan_stops_before_transition_when_global_capacity_is_full() {
    let fixture = setup_control_fixture().await;
    fixture
        .state
        .execution_plan_repo
        .set_halt_mode(&fixture.current_plan_id, ExecutionPlanHaltMode::Paused)
        .await
        .unwrap();
    let paused_task = create_paused_plan_task_with_previous_status(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Capacity gated resume task",
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_global_max_concurrent(1);
    execution_state.set_running_count(1);

    let service = ExecutionPlanControlService::new(&fixture.state, execution_state, None);

    let outcome = service.resume_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.execution_plan_id, fixture.current_plan_id);
    assert_eq!(outcome.affected_count, 0);
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Running
    );
    assert_eq!(
        stored_task(&fixture.state, &paused_task.id)
            .await
            .internal_status,
        InternalStatus::Paused
    );
}

#[tokio::test]
async fn resume_plan_stops_before_transition_when_project_capacity_is_full() {
    let fixture = setup_control_fixture().await;
    fixture
        .state
        .execution_plan_repo
        .set_halt_mode(&fixture.current_plan_id, ExecutionPlanHaltMode::Paused)
        .await
        .unwrap();
    fixture
        .state
        .execution_settings_repo
        .update_settings(
            Some(&fixture.project_id),
            &ExecutionSettings {
                max_concurrent_tasks: 0,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();
    let paused_task = create_paused_plan_task_with_previous_status(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Project capacity gated resume task",
    )
    .await;

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let outcome = service.resume_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.execution_plan_id, fixture.current_plan_id);
    assert_eq!(outcome.affected_count, 0);
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Running
    );
    assert_eq!(
        stored_task(&fixture.state, &paused_task.id)
            .await
            .internal_status,
        InternalStatus::Paused
    );
}

#[tokio::test]
async fn resume_plan_ignores_non_paused_plan_tasks() {
    let fixture = setup_control_fixture().await;
    fixture
        .state
        .execution_plan_repo
        .set_halt_mode(&fixture.current_plan_id, ExecutionPlanHaltMode::Paused)
        .await
        .unwrap();
    let executing_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Already executing task",
    )
    .await;
    let paused_task = create_paused_plan_task_with_previous_status(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Paused task to resume",
    )
    .await;

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let outcome = service.resume_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.execution_plan_id, fixture.current_plan_id);
    assert_eq!(outcome.affected_count, 1);
    assert_eq!(
        stored_task(&fixture.state, &executing_task.id)
            .await
            .internal_status,
        InternalStatus::Executing,
        "resume must not rewrite tasks that are no longer paused"
    );
    assert_eq!(
        stored_task(&fixture.state, &paused_task.id)
            .await
            .internal_status,
        InternalStatus::Failed,
        "restorable paused task should still reach entry actions"
    );
}

#[tokio::test]
async fn resume_plan_restores_paused_active_task_and_prepares_entry_actions() {
    let fixture = setup_control_fixture().await;
    fixture
        .state
        .execution_plan_repo
        .set_halt_mode(&fixture.current_plan_id, ExecutionPlanHaltMode::Paused)
        .await
        .unwrap();
    let paused_task = create_paused_plan_task_with_previous_status(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Restorable execution-plan task",
    )
    .await;

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let outcome = service.resume_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.execution_plan_id, fixture.current_plan_id);
    assert_eq!(outcome.affected_count, 1);
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Running
    );

    let stored = stored_task(&fixture.state, &paused_task.id).await;
    assert_eq!(
        stored.internal_status,
        InternalStatus::Failed,
        "mock execution startup should prove resumed entry actions ran"
    );
    assert!(
        PauseReason::from_task_metadata(stored.metadata.as_deref()).is_none(),
        "resume should clear pause metadata before entry actions"
    );
    assert!(
        stored
            .metadata
            .as_deref()
            .is_some_and(|metadata| metadata.contains("\"trigger_origin\":\"resume\"")),
        "resume should mark task metadata for resumed entry actions"
    );
}

#[tokio::test]
async fn resume_plan_skips_paused_task_without_restore_status() {
    let fixture = setup_control_fixture().await;
    fixture
        .state
        .execution_plan_repo
        .set_halt_mode(&fixture.current_plan_id, ExecutionPlanHaltMode::Paused)
        .await
        .unwrap();
    let paused_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Paused,
        "Paused task without restore status",
    )
    .await;

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let outcome = service.resume_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.execution_plan_id, fixture.current_plan_id);
    assert_eq!(outcome.affected_count, 0);
    assert_eq!(
        stored_task(&fixture.state, &paused_task.id)
            .await
            .internal_status,
        InternalStatus::Paused
    );
}

#[tokio::test]
async fn resume_plan_skips_paused_task_when_restore_status_lookup_fails() {
    let mut fixture = setup_control_fixture().await;
    fixture
        .state
        .execution_plan_repo
        .set_halt_mode(&fixture.current_plan_id, ExecutionPlanHaltMode::Paused)
        .await
        .unwrap();
    let paused_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Paused,
        "Paused task with failing restore lookup",
    )
    .await;
    fail_status_history_for_restore(&mut fixture);

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let outcome = service.resume_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.execution_plan_id, fixture.current_plan_id);
    assert_eq!(outcome.affected_count, 0);
    assert_eq!(
        stored_task(&fixture.state, &paused_task.id)
            .await
            .internal_status,
        InternalStatus::Paused
    );
}

#[tokio::test]
async fn resume_plan_continues_after_transition_failure() {
    let mut fixture = setup_control_fixture().await;
    fixture
        .state
        .execution_plan_repo
        .set_halt_mode(&fixture.current_plan_id, ExecutionPlanHaltMode::Paused)
        .await
        .unwrap();
    let failing_task = create_paused_plan_task_with_previous_status(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Failing resume task",
    )
    .await;
    let succeeding_task = create_paused_plan_task_with_previous_status(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Succeeding resume task",
    )
    .await;
    fail_transition_for(&mut fixture, failing_task.id.clone());

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let outcome = service.resume_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.execution_plan_id, fixture.current_plan_id);
    assert_eq!(outcome.affected_count, 1);
    assert_eq!(
        stored_task(&fixture.state, &failing_task.id)
            .await
            .internal_status,
        InternalStatus::Paused,
        "failed resume transition should leave the task paused"
    );
    assert_eq!(
        stored_task(&fixture.state, &succeeding_task.id)
            .await
            .internal_status,
        InternalStatus::Failed,
        "later paused tasks should still resume through entry actions"
    );
}

#[tokio::test]
async fn resume_plan_skips_paused_task_with_non_agent_active_restore_status() {
    let fixture = setup_control_fixture().await;
    fixture
        .state
        .execution_plan_repo
        .set_halt_mode(&fixture.current_plan_id, ExecutionPlanHaltMode::Paused)
        .await
        .unwrap();
    let paused_task = create_paused_plan_task_with_previous_status(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Ready,
        "Non-agent restore task",
    )
    .await;

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let outcome = service.resume_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.execution_plan_id, fixture.current_plan_id);
    assert_eq!(outcome.affected_count, 0);
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Running
    );
    assert_eq!(
        stored_task(&fixture.state, &paused_task.id)
            .await
            .internal_status,
        InternalStatus::Paused
    );
}

#[tokio::test]
async fn pause_plan_rejects_missing_ideation_session() {
    let fixture = setup_control_fixture().await;

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let result = service
        .pause_plan(ExecutionPlanControlScope {
            project_id: fixture.project_id.clone(),
            session_id: IdeationSessionId::from_string("missing-session".to_string()),
            execution_plan_id: None,
        })
        .await;

    match result {
        Err(AppError::NotFound(message)) => {
            assert!(
                message.contains("Ideation session not found"),
                "unexpected missing-session error: {message}"
            );
        }
        other => panic!("expected missing-session not found error, got {other:?}"),
    }
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Running
    );
}

#[tokio::test]
async fn pause_plan_rejects_session_from_other_project() {
    let fixture = setup_control_fixture().await;
    let other_project = fixture
        .state
        .project_repo
        .create(Project::new(
            "Other project".to_string(),
            "/tmp/ralphx-other-plan-project".to_string(),
        ))
        .await
        .unwrap();

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let result = service
        .pause_plan(ExecutionPlanControlScope {
            project_id: other_project.id,
            session_id: fixture.session_id.clone(),
            execution_plan_id: Some(fixture.current_plan_id.clone()),
        })
        .await;

    match result {
        Err(AppError::Validation(message)) => {
            assert!(
                message.contains("belongs to project"),
                "unexpected project mismatch error: {message}"
            );
        }
        other => panic!("expected project mismatch validation error, got {other:?}"),
    }
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Running
    );
}

#[tokio::test]
async fn pause_plan_rejects_session_without_active_execution_plan() {
    let fixture = setup_control_fixture().await;
    fixture
        .state
        .execution_plan_repo
        .mark_superseded(&fixture.current_plan_id)
        .await
        .unwrap();

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let result = service.pause_plan(scope(&fixture)).await;

    match result {
        Err(AppError::NotFound(message)) => {
            assert!(
                message.contains("Active execution plan not found"),
                "unexpected missing-plan error: {message}"
            );
        }
        other => panic!("expected missing-plan not found error, got {other:?}"),
    }
}

#[tokio::test]
async fn pause_plan_rejects_stale_explicit_plan_id_without_mutation() {
    let fixture = setup_control_fixture().await;
    let current_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Current active attempt",
    )
    .await;
    let stale_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.old_plan_id,
        InternalStatus::Executing,
        "Superseded attempt",
    )
    .await;

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let result = service
        .pause_plan(ExecutionPlanControlScope {
            project_id: fixture.project_id.clone(),
            session_id: fixture.session_id.clone(),
            execution_plan_id: Some(fixture.old_plan_id.clone()),
        })
        .await;

    match result {
        Err(AppError::Validation(message)) => {
            assert!(
                message.contains("active execution plan"),
                "stale explicit plan id should be rejected as non-current, got: {message}"
            );
        }
        other => panic!("expected stale explicit plan id validation error, got {other:?}"),
    }
    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Running
    );
    assert_eq!(
        plan_halt_mode(&fixture.state, &fixture.old_plan_id).await,
        ExecutionPlanHaltMode::Running
    );

    let current = stored_task(&fixture.state, &current_task.id).await;
    assert_eq!(current.internal_status, InternalStatus::Executing);
    assert!(
        PauseReason::from_task_metadata(current.metadata.as_deref()).is_none(),
        "current active attempt must not receive stale-id pause metadata"
    );
    let stale = stored_task(&fixture.state, &stale_task.id).await;
    assert_eq!(stale.internal_status, InternalStatus::Executing);
    assert!(
        PauseReason::from_task_metadata(stale.metadata.as_deref()).is_none(),
        "superseded attempt must not receive pause metadata"
    );
}

#[tokio::test]
async fn pause_plan_propagates_transition_failure_without_stranding_task() {
    let mut fixture = setup_control_fixture().await;
    let task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Transition failure task",
    )
    .await;
    fail_transition_for(&mut fixture, task.id.clone());

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let result = service.pause_plan(scope(&fixture)).await;

    assert!(result.is_err());

    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Running
    );

    let stored = stored_task(&fixture.state, &task.id).await;
    assert_eq!(stored.internal_status, InternalStatus::Executing);
    assert!(
        PauseReason::from_task_metadata(stored.metadata.as_deref()).is_none(),
        "failed pause transition must not leave pause metadata behind"
    );
}

#[tokio::test]
async fn stop_plan_propagates_transition_failure_without_stranding_task() {
    let mut fixture = setup_control_fixture().await;
    let task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Transition failure stop task",
    )
    .await;
    fail_transition_for(&mut fixture, task.id.clone());

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let result = service.stop_plan(scope(&fixture)).await;

    assert!(result.is_err());

    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Running
    );

    let stored = stored_task(&fixture.state, &task.id).await;
    assert_eq!(stored.internal_status, InternalStatus::Executing);
    assert!(
        stored.metadata.is_none(),
        "failed stop transition must not write stop metadata"
    );
}

#[tokio::test]
async fn pause_plan_sets_halt_mode_before_plan_task_query() {
    let mut fixture = setup_control_fixture().await;
    let task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Query failure pause task",
    )
    .await;
    fail_plan_task_query(&mut fixture);

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let result = service.pause_plan(scope(&fixture)).await;

    assert!(result.is_err());

    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Paused
    );

    let stored = stored_task(&fixture.state, &task.id).await;
    assert_eq!(stored.internal_status, InternalStatus::Executing);
    assert!(
        PauseReason::from_task_metadata(stored.metadata.as_deref()).is_none(),
        "failed plan task query must not leave pause metadata behind"
    );
}

#[tokio::test]
async fn stop_plan_sets_halt_mode_before_plan_task_query() {
    let mut fixture = setup_control_fixture().await;
    let task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Query failure stop task",
    )
    .await;
    fail_plan_task_query(&mut fixture);

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let result = service.stop_plan(scope(&fixture)).await;

    assert!(result.is_err());

    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Stopped
    );

    let stored = stored_task(&fixture.state, &task.id).await;
    assert_eq!(stored.internal_status, InternalStatus::Executing);
    assert!(
        stored.metadata.is_none(),
        "failed plan task query must not write stop metadata"
    );
}

#[tokio::test]
async fn resume_plan_preserves_halt_mode_when_plan_task_query_fails() {
    let mut fixture = setup_control_fixture().await;
    fixture
        .state
        .execution_plan_repo
        .set_halt_mode(&fixture.current_plan_id, ExecutionPlanHaltMode::Paused)
        .await
        .unwrap();
    let task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Paused,
        "Query failure resume task",
    )
    .await;
    fail_plan_task_query(&mut fixture);

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let result = service.resume_plan(scope(&fixture)).await;

    assert!(result.is_err());

    assert_eq!(
        current_plan_halt_mode(&fixture).await,
        ExecutionPlanHaltMode::Paused
    );

    let stored = stored_task(&fixture.state, &task.id).await;
    assert_eq!(stored.internal_status, InternalStatus::Paused);
}

#[tokio::test]
async fn pause_plan_applies_agent_active_guard_in_both_directions() {
    let fixture = setup_control_fixture().await;
    let ready_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Ready,
        "Ready task remains queued",
    )
    .await;
    let executing_task = create_plan_task(
        &fixture.state,
        &fixture.project_id,
        &fixture.session_id,
        &fixture.current_plan_id,
        InternalStatus::Executing,
        "Executing task is paused",
    )
    .await;

    let service =
        ExecutionPlanControlService::new(&fixture.state, Arc::new(ExecutionState::new()), None);

    let outcome = service.pause_plan(scope(&fixture)).await.unwrap();

    assert_eq!(outcome.affected_count, 1);
    assert_eq!(
        stored_task(&fixture.state, &ready_task.id)
            .await
            .internal_status,
        InternalStatus::Ready,
        "non-agent-active tasks must not be transitioned"
    );
    assert_eq!(
        stored_task(&fixture.state, &executing_task.id)
            .await
            .internal_status,
        InternalStatus::Paused,
        "agent-active tasks must transition through the control service"
    );
}

struct FailingTaskRepository {
    inner: Arc<dyn TaskRepository>,
    transition_task_id: Option<TaskId>,
    fail_plan_task_query: bool,
    fail_status_history: bool,
}

#[async_trait]
impl TaskRepository for FailingTaskRepository {
    async fn create(&self, task: Task) -> AppResult<Task> {
        self.inner.create(task).await
    }

    async fn get_by_id(&self, id: &TaskId) -> AppResult<Option<Task>> {
        self.inner.get_by_id(id).await
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<Task>> {
        self.inner.get_by_project(project_id).await
    }

    async fn update(&self, task: &Task) -> AppResult<()> {
        self.inner.update(task).await
    }

    async fn update_with_expected_status(
        &self,
        task: &Task,
        expected_status: InternalStatus,
    ) -> AppResult<bool> {
        if self.transition_task_id.as_ref() == Some(&task.id) {
            return Err(AppError::Validation(
                "forced transition failure".to_string(),
            ));
        }
        self.inner
            .update_with_expected_status(task, expected_status)
            .await
    }

    async fn update_metadata(&self, id: &TaskId, metadata: Option<String>) -> AppResult<()> {
        self.inner.update_metadata(id, metadata).await
    }

    async fn delete(&self, id: &TaskId) -> AppResult<()> {
        self.inner.delete(id).await
    }

    async fn get_by_status(
        &self,
        project_id: &ProjectId,
        status: InternalStatus,
    ) -> AppResult<Vec<Task>> {
        self.inner.get_by_status(project_id, status).await
    }

    async fn persist_status_change(
        &self,
        id: &TaskId,
        from: InternalStatus,
        to: InternalStatus,
        trigger: &str,
    ) -> AppResult<String> {
        self.inner
            .persist_status_change(id, from, to, trigger)
            .await
    }

    async fn get_status_history(&self, id: &TaskId) -> AppResult<Vec<StatusTransition>> {
        if self.fail_status_history {
            return Err(AppError::Validation(
                "forced status history failure".to_string(),
            ));
        }
        self.inner.get_status_history(id).await
    }

    async fn get_status_history_batch(
        &self,
        task_ids: &[TaskId],
    ) -> AppResult<HashMap<TaskId, Vec<StatusTransition>>> {
        self.inner.get_status_history_batch(task_ids).await
    }

    async fn get_status_entered_at(
        &self,
        task_id: &TaskId,
        status: InternalStatus,
    ) -> AppResult<Option<DateTime<Utc>>> {
        self.inner.get_status_entered_at(task_id, status).await
    }

    async fn get_status_last_entered_at(
        &self,
        task_id: &TaskId,
        status: InternalStatus,
    ) -> AppResult<Option<DateTime<Utc>>> {
        self.inner.get_status_last_entered_at(task_id, status).await
    }

    async fn get_next_executable(&self, project_id: &ProjectId) -> AppResult<Option<Task>> {
        self.inner.get_next_executable(project_id).await
    }

    async fn get_by_ideation_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<Task>> {
        if self.fail_plan_task_query {
            return Err(AppError::Validation(
                "forced plan task query failure".to_string(),
            ));
        }
        self.inner.get_by_ideation_session(session_id).await
    }

    async fn get_by_project_filtered(
        &self,
        project_id: &ProjectId,
        include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        self.inner
            .get_by_project_filtered(project_id, include_archived)
            .await
    }

    async fn archive(&self, task_id: &TaskId) -> AppResult<Task> {
        self.inner.archive(task_id).await
    }

    async fn restore(&self, task_id: &TaskId) -> AppResult<Task> {
        self.inner.restore(task_id).await
    }

    async fn get_archived_count(
        &self,
        project_id: &ProjectId,
        ideation_session_id: Option<&str>,
    ) -> AppResult<u32> {
        self.inner
            .get_archived_count(project_id, ideation_session_id)
            .await
    }

    async fn list_paginated(
        &self,
        project_id: &ProjectId,
        statuses: Option<Vec<InternalStatus>>,
        offset: u32,
        limit: u32,
        include_archived: bool,
        ideation_session_id: Option<&str>,
        execution_plan_id: Option<&str>,
        categories: Option<&[String]>,
    ) -> AppResult<Vec<Task>> {
        self.inner
            .list_paginated(
                project_id,
                statuses,
                offset,
                limit,
                include_archived,
                ideation_session_id,
                execution_plan_id,
                categories,
            )
            .await
    }

    async fn count_tasks(
        &self,
        project_id: &ProjectId,
        include_archived: bool,
        ideation_session_id: Option<&str>,
        execution_plan_id: Option<&str>,
    ) -> AppResult<u32> {
        self.inner
            .count_tasks(
                project_id,
                include_archived,
                ideation_session_id,
                execution_plan_id,
            )
            .await
    }

    async fn search(
        &self,
        project_id: &ProjectId,
        query: &str,
        include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        self.inner.search(project_id, query, include_archived).await
    }

    async fn get_oldest_ready_task(&self) -> AppResult<Option<Task>> {
        self.inner.get_oldest_ready_task().await
    }

    async fn get_oldest_ready_tasks(&self, limit: u32) -> AppResult<Vec<Task>> {
        self.inner.get_oldest_ready_tasks(limit).await
    }

    async fn get_stale_ready_tasks(&self, threshold_secs: u64) -> AppResult<Vec<Task>> {
        self.inner.get_stale_ready_tasks(threshold_secs).await
    }

    async fn update_latest_state_history_metadata(
        &self,
        task_id: &TaskId,
        metadata: &StateHistoryMetadata,
    ) -> AppResult<()> {
        self.inner
            .update_latest_state_history_metadata(task_id, metadata)
            .await
    }

    async fn has_task_in_states(
        &self,
        project_id: &ProjectId,
        statuses: &[InternalStatus],
    ) -> AppResult<bool> {
        self.inner.has_task_in_states(project_id, statuses).await
    }
}
