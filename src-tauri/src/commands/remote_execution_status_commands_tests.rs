use std::sync::Arc;

use super::remote_execution_status_commands::get_remote_execution_status_for_state;
use crate::application::AppState;
use crate::commands::execution_commands::{ActiveProjectState, ExecutionState};
use crate::domain::entities::app_state::ExecutionHaltMode;
use crate::domain::entities::{ChatContextType, InternalStatus, Project, ProjectId, Task};
use crate::domain::services::{MemoryRunningAgentRegistry, RunningAgentKey, RunningAgentRegistry};

#[tokio::test]
async fn remote_execution_status_reports_host_state_without_mutating_registry_or_cache() {
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(4));
    execution_state.pause();
    execution_state.set_running_count(7);
    let active_project_state = Arc::new(ActiveProjectState::new());
    let mut app_state = AppState::new_test();

    let project = app_state
        .project_repo
        .create(Project::new(
            "Remote status project".to_string(),
            "/test/remote-status".to_string(),
        ))
        .await
        .expect("project seed succeeds");
    active_project_state.set(Some(project.id.clone())).await;

    let mut running_task = Task::new(project.id.clone(), "Running task".to_string());
    running_task.internal_status = InternalStatus::Executing;
    let running_task = app_state
        .task_repo
        .create(running_task)
        .await
        .expect("running task seed succeeds");

    let mut queued_task = Task::new(project.id.clone(), "Queued task".to_string());
    queued_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(queued_task)
        .await
        .expect("queued task seed succeeds");

    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    registry
        .set_running(RunningAgentKey::new(
            ChatContextType::TaskExecution.to_string(),
            running_task.id.as_str(),
        ))
        .await;
    app_state.running_agent_registry = registry.clone();
    app_state
        .app_state_repo
        .set_execution_halt_mode(ExecutionHaltMode::Stopped)
        .await
        .expect("halt mode seed succeeds");

    let before = registry.list_all().await;
    let status = get_remote_execution_status_for_state(
        None,
        &execution_state,
        &app_state,
        &active_project_state,
    )
    .await
    .expect("spawn-free status read succeeds");
    let after = registry.list_all().await;

    assert_eq!(status.halt_mode, "stopped");
    assert!(status.is_paused);
    assert_eq!(status.running_count, 1);
    assert_eq!(status.queued_count, 1);
    assert_eq!(
        execution_state.running_count(),
        7,
        "cache remains untouched"
    );
    assert_eq!(registry_snapshot(&after), registry_snapshot(&before));
}

#[tokio::test]
async fn remote_execution_status_propagates_repository_errors() {
    let execution_state = Arc::new(ExecutionState::new());
    let active_project_state = Arc::new(ActiveProjectState::new());
    let app_state = AppState::new_sqlite_test();
    let project_id = ProjectId::from_string("remote-status-fail-closed".to_string());
    active_project_state.set(Some(project_id)).await;
    app_state
        .db
        .run(|connection| {
            connection.execute("DROP TABLE ideation_sessions", [])?;
            Ok(())
        })
        .await
        .expect("fault injection removes the pending-session source");

    let error = get_remote_execution_status_for_state(
        None,
        &execution_state,
        &app_state,
        &active_project_state,
    )
    .await
    .expect_err("repository failure must fail the snapshot closed");

    assert!(error.contains("ideation_sessions"));
}

fn registry_snapshot(
    entries: &[(RunningAgentKey, crate::domain::services::RunningAgentInfo)],
) -> Vec<(RunningAgentKey, u32, String)> {
    let mut snapshot = entries
        .iter()
        .map(|(key, info)| (key.clone(), info.pid, info.agent_run_id.clone()))
        .collect::<Vec<_>>();
    snapshot.sort_by(|left, right| {
        (&left.0.context_type, &left.0.context_id)
            .cmp(&(&right.0.context_type, &right.0.context_id))
    });
    snapshot
}
