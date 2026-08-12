use super::*;
pub use crate::domain::execution::{
    ExecutionCapacitySummary, ExecutionLaneUsage, RunningIdeationSession, RunningProcess,
    RunningProcessesResponse, RunningWorkspaceSession, DEFAULT_WORKSPACE_MAX_CONCURRENT,
};

#[doc(hidden)]
pub use crate::application::execution_state::context_matches_running_status_for_gc;

pub(super) async fn prune_stale_execution_registry_entries(
    app_state: &AppState,
    execution_state: &ExecutionState,
) {
    let entries = app_state.running_agent_registry.list_all().await;
    if entries.is_empty() {
        return;
    }

    let engine = crate::application::PruneEngine::new(
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.project_repo),
        Some(Arc::clone(&app_state.interactive_process_registry)),
    );

    let mut pruned_any = false;

    for (key, info) in &entries {
        let context_type = match ChatContextType::from_str(&key.context_type) {
            Ok(ct) => ct,
            Err(_) => continue,
        };

        if !uses_execution_slot(context_type) {
            continue;
        }

        // Compute pid liveness once; both the IPR check and staleness evaluation use it.
        let pid_alive = crate::domain::services::is_process_alive(info.pid);

        // PID-verified IPR check: skip if interactive process is alive; remove stale
        // IPR entries (PID dead) so reconciliation is not blocked forever.
        if engine.check_ipr_skip(key, pid_alive).await {
            continue;
        }

        if engine.evaluate_and_prune(key, info, pid_alive).await {
            // Clean up any interactive idle slot tracking for this pruned entry
            // so ghost entries don't persist in interactive_idle_slots.
            let slot_key = format!("{}/{}", key.context_type, key.context_id);
            execution_state.remove_interactive_slot(&slot_key);
            pruned_any = true;
        }
    }

    // Correct the running count if entries were pruned.  The GC runs every ~5s so
    // this keeps the slot counter accurate between 30s reconciliation cycles (Bug 3).
    if pruned_any {
        let remaining = app_state.running_agent_registry.list_all().await;
        let mut active_slots = 0u32;
        for (key, _) in &remaining {
            let context_type = match ChatContextType::from_str(&key.context_type) {
                Ok(value) => value,
                Err(_) => continue,
            };

            if !uses_execution_slot(context_type) {
                continue;
            }

            if matches!(context_type, ChatContextType::Ideation) {
                active_slots += 1;
                continue;
            }

            let task_id = TaskId::from_string(key.context_id.clone());
            let task = match app_state.task_repo.get_by_id(&task_id).await {
                Ok(Some(task)) => task,
                _ => continue,
            };

            if task.archived_at.is_some()
                || !context_matches_running_status_for_gc(context_type, task.internal_status)
            {
                continue;
            }

            active_slots += 1;
        }
        let idle_count = remaining
            .iter()
            .filter(|(k, _)| {
                let slot_key = format!("{}/{}", k.context_type, k.context_id);
                execution_state.is_interactive_idle(&slot_key)
            })
            .count() as u32;
        execution_state.set_running_count(active_slots.saturating_sub(idle_count));
    }
}
