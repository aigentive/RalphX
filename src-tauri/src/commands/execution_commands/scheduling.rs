use super::*;

pub(super) async fn schedule_ready_tasks_for_project(
    app_state: &AppState,
    execution_state: Arc<ExecutionState>,
    project_id: Option<ProjectId>,
) {
    let scheduler =
        Arc::new(app_state.build_task_scheduler_for_runtime(Arc::clone(&execution_state), None));
    scheduler.set_self_ref(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);
    scheduler.set_active_project(project_id).await;
    scheduler.try_schedule_ready_tasks().await;
}
