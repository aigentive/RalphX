use std::sync::Arc;

use super::startup_runtime_builders::*;
use crate::application::app_state::ApplicationExecutionState as ExecutionState;
use crate::application::AppState;
use crate::domain::entities::{ExecutionPlan, IdeationSession, InternalStatus, Project, Task};

#[tokio::test]
async fn startup_scheduler_skips_ready_task_from_superseded_execution_plan() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new(
        "Startup Scheduler Project".into(),
        "/tmp/startup-scheduler".into(),
    );
    let project_id = project.id.clone();
    app_state.project_repo.create(project).await.unwrap();

    let session = IdeationSession::new(project_id.clone());
    let session = app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let stale_plan = app_state
        .execution_plan_repo
        .create(ExecutionPlan::new(session.id.clone()))
        .await
        .unwrap();
    app_state
        .execution_plan_repo
        .mark_superseded(&stale_plan.id)
        .await
        .unwrap();

    let mut stale_task = Task::new(project_id, "stale ready task".into());
    stale_task.internal_status = InternalStatus::Ready;
    stale_task.execution_plan_id = Some(stale_plan.id.clone());
    let stale_task_id = stale_task.id.clone();
    app_state.task_repo.create(stale_task).await.unwrap();

    let scheduler = build_startup_task_scheduler(StartupSchedulerDeps {
        execution_state,
        project_repo: Arc::clone(&app_state.project_repo),
        task_repo: Arc::clone(&app_state.task_repo),
        task_dependency_repo: Arc::clone(&app_state.task_dependency_repo),
        artifact_repo: Arc::clone(&app_state.artifact_repo),
        chat_message_repo: Arc::clone(&app_state.chat_message_repo),
        chat_attachment_repo: Arc::clone(&app_state.chat_attachment_repo),
        conversation_repo: Arc::clone(&app_state.chat_conversation_repo),
        agent_run_repo: Arc::clone(&app_state.agent_run_repo),
        ideation_session_repo: Arc::clone(&app_state.ideation_session_repo),
        activity_event_repo: Arc::clone(&app_state.activity_event_repo),
        message_queue: Arc::clone(&app_state.message_queue),
        running_agent_registry: Arc::clone(&app_state.running_agent_registry),
        memory_event_repo: Arc::clone(&app_state.memory_event_repo),
        agent_clients: app_state.agent_client_bundle(),
        agent_lane_settings_repo: Arc::clone(&app_state.agent_lane_settings_repo),
        agent_provider_settings_repo: Arc::clone(&app_state.agent_provider_settings_repo),
        plan_branch_repo: Arc::clone(&app_state.plan_branch_repo),
        execution_plan_repo: Arc::clone(&app_state.execution_plan_repo),
        interactive_process_registry: Arc::clone(&app_state.interactive_process_registry),
        github_service: None,
        pr_poller_registry: Arc::clone(&app_state.pr_poller_registry),
    });

    scheduler.try_schedule_ready_tasks().await;

    let stored = app_state
        .task_repo
        .get_by_id(&stale_task_id)
        .await
        .unwrap()
        .expect("stale task should remain persisted");
    assert_eq!(
        stored.internal_status,
        InternalStatus::Ready,
        "startup-built scheduler must not admit superseded-plan ready tasks"
    );
}

#[test]
fn startup_scheduler_carries_lane_and_provider_settings() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let scheduler = build_startup_task_scheduler_concrete(StartupSchedulerDeps {
        execution_state,
        project_repo: Arc::clone(&app_state.project_repo),
        task_repo: Arc::clone(&app_state.task_repo),
        task_dependency_repo: Arc::clone(&app_state.task_dependency_repo),
        artifact_repo: Arc::clone(&app_state.artifact_repo),
        chat_message_repo: Arc::clone(&app_state.chat_message_repo),
        chat_attachment_repo: Arc::clone(&app_state.chat_attachment_repo),
        conversation_repo: Arc::clone(&app_state.chat_conversation_repo),
        agent_run_repo: Arc::clone(&app_state.agent_run_repo),
        ideation_session_repo: Arc::clone(&app_state.ideation_session_repo),
        activity_event_repo: Arc::clone(&app_state.activity_event_repo),
        message_queue: Arc::clone(&app_state.message_queue),
        running_agent_registry: Arc::clone(&app_state.running_agent_registry),
        memory_event_repo: Arc::clone(&app_state.memory_event_repo),
        agent_clients: app_state.agent_client_bundle(),
        agent_lane_settings_repo: Arc::clone(&app_state.agent_lane_settings_repo),
        agent_provider_settings_repo: Arc::clone(&app_state.agent_provider_settings_repo),
        plan_branch_repo: Arc::clone(&app_state.plan_branch_repo),
        execution_plan_repo: Arc::clone(&app_state.execution_plan_repo),
        interactive_process_registry: Arc::clone(&app_state.interactive_process_registry),
        github_service: None,
        pr_poller_registry: Arc::clone(&app_state.pr_poller_registry),
    });

    assert!(scheduler.agent_lane_settings_repo.is_some());
    assert!(scheduler.agent_provider_settings_repo.is_some());
}
