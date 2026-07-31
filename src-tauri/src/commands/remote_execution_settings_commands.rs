//! The spawn-free execution-settings write for the remote facade.
//!
//! # The asymmetry this closes
//!
//! `get_execution_settings` is registered, so a paired client already SHOWS the host's real
//! execution settings. `update_execution_settings` is not, so saving them failed silently.
//! A pane that displays live values and cannot persist them is worse than one that is
//! honestly read-only, because the user has no way to tell which of the two they are looking
//! at.
//!
//! # Why the local command cannot simply be registered
//!
//! Its `Elevated` class is EARNED, not a conservative module default. Two of its effects
//! reach spawn authority:
//!
//! ```text
//! update_execution_settings
//!   -> schedule_ready_tasks_for_project        (max_concurrent_tasks raised)
//!        -> scheduler.try_schedule_ready_tasks  [Ready -> Executing, launches queued work]
//!   -> PendingSessionDrainService              (project_ideation_max raised)
//!        -> build_chat_service_with_execution_state  [SPAWN: provider CLI]
//! ```
//!
//! `Elevated` is unregistrable, and no amount of scope tightening changes that: the gate is a
//! capability gate, not a scope gate (`detector_c_floors_process_spawn_authority`). Something
//! has to change about the reachable sinks, which is what this split does.
//!
//! # What this command does and deliberately does not do
//!
//! Does: persist the settings, and sync the two `ExecutionState` atomics so the running
//! process does not keep scheduling against a stale cap.
//!
//! Does NOT: kick the scheduler, and does NOT drain pending ideation sessions.
//!
//! That is a real behavioural difference and it is the price of the split. Raising capacity
//! from a remote client does not *immediately* launch queued work; the new cap is honoured by
//! the next scheduling pass, which any task transition drives (exiting an agent-active state
//! decrements capacity and calls `try_schedule_ready_tasks` — the ledger records that edge).
//! On a fully idle host with Ready tasks waiting, remote-raised capacity therefore takes
//! effect on the next transition rather than instantly. That is a latency difference, not a
//! lost setting, and it is strictly better than the current behaviour of not saving at all.
//!
//! # Class
//!
//! `AgentControl`, not `Operate`. Persisting a higher cap seeds state a background loop turns
//! into a spawn, which is exactly the `seedsSpawnTriggeringState` capability — the ledger's
//! rule is that classification traces downstream authority, not immediate action. It
//! therefore requires `ui:agent`, the per-device grant that is off by default.
//!
//! # No event emission
//!
//! The local command emits `settings:execution:updated` for the local UI.
//! `settings:execution:updated` is not in the remote event classification table, and emitting
//! an unclassified name from a registered command is exactly the drift the table exists to
//! prevent. The updated settings are returned instead, so the caller updates its own cache
//! from the response.

use std::sync::Arc;

use tauri::State;

use crate::application::AppState;
use crate::commands::execution_commands::{
    ExecutionSettingsResponse, ExecutionState, UpdateExecutionSettingsInput,
};
use crate::domain::entities::ProjectId;
use crate::domain::execution::ExecutionSettings;

/// Persists the host's execution settings without arming any queued work.
///
/// # Errors
///
/// Propagates the execution-settings repository error. A failed write must surface: the
/// client renders the returned values as the new truth, so a swallowed error would leave the
/// pane showing settings the host never stored.
#[tauri::command]
pub async fn update_remote_execution_settings(
    project_id: Option<String>,
    input: UpdateExecutionSettingsInput,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
) -> Result<ExecutionSettingsResponse, String> {
    update_remote_execution_settings_for_state(
        project_id,
        input,
        execution_state.inner(),
        app_state.inner(),
    )
    .await
}

#[doc(hidden)]
pub async fn update_remote_execution_settings_for_state(
    project_id: Option<String>,
    input: UpdateExecutionSettingsInput,
    execution_state: &Arc<ExecutionState>,
    app_state: &AppState,
) -> Result<ExecutionSettingsResponse, String> {
    let project_id = project_id.map(ProjectId::from_string);
    let settings = ExecutionSettings {
        max_concurrent_tasks: input.max_concurrent_tasks,
        project_ideation_max: input.project_ideation_max,
        auto_commit: input.auto_commit,
        pause_on_failure: input.pause_on_failure,
        agent_workspace_pr_autofix_default: input.agent_workspace_pr_autofix_default,
        agent_workspace_pr_auto_merge_default: input.agent_workspace_pr_auto_merge_default,
    };

    let updated = app_state
        .execution_settings_repo
        .update_settings(project_id.as_ref(), &settings)
        .await
        .map_err(|error| error.to_string())?;

    // Sync the in-process caps AFTER the write succeeds. Doing it first would leave the
    // running process scheduling against a cap the database never accepted.
    execution_state.set_max_concurrent(updated.max_concurrent_tasks);
    execution_state.set_project_ideation_max(updated.project_ideation_max);

    Ok(ExecutionSettingsResponse::from(updated))
}
