use std::sync::Arc;

use tauri::AppHandle;

use crate::application::harness_runtime_registry::default_scheduler_ready_settle_ms;
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::state_machine::services::TaskScheduler;

/// Spawn the canonical ready-task scheduler after proposal acceptance.
///
/// This keeps all accept-plan entry points on the same scheduler construction path
/// so PR mode, poller wiring, and self-ref behavior stay aligned.
pub fn spawn_ready_task_scheduler_if_needed(
    app_state: &AppState,
    execution_state: Arc<ExecutionState>,
    _app_handle: Option<AppHandle>,
    any_ready_tasks: bool,
) {
    if !any_ready_tasks {
        return;
    }

    let scheduler = Arc::new(app_state.build_task_scheduler_for_runtime(execution_state, None));
    scheduler.set_self_ref(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);

    let settle_ms = default_scheduler_ready_settle_ms();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(settle_ms)).await;
        scheduler.try_schedule_ready_tasks().await;
    });
}
