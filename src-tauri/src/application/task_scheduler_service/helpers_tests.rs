use super::*;
use crate::application::AppState;
use crate::commands::ExecutionState;
use tauri::test::MockRuntime;

fn scheduler_for_state(state: &AppState) -> TaskSchedulerService<MockRuntime> {
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
