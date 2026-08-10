//! The spawn-free execution-status read for the remote facade.
//!
//! # The asymmetry this closes
//!
//! Remote clients can update execution settings, but they cannot read the host scheduler's
//! actual status. Falling back to optimistic defaults makes a stopped scheduler look healthy
//! and prevents the queue-halt banner from rendering.
//!
//! # Why the local command cannot simply be registered
//!
//! The exact refused path is `get_execution_status` →
//! `prune_stale_execution_registry_entries` → `is_process_alive` → process-kill/inspection CLI
//! resolvers (`resolve_tasklist_cli_path` on the detector-c trace, plus platform kill resolvers).
//! That spawn path keeps the local command refused. The local command also synchronizes runtime
//! quotas and caches its computed running count, so it is not a read-only facade operation.
//!
//! # What this command deliberately does not do
//!
//! This twin resolves the effective project without syncing quotas, reads registry rows as-is
//! without pruning them, and computes the global running count without writing it back to
//! `ExecutionState`. Host-local reads remain responsible for stale-row maintenance. It shares the
//! status projection with the local command so response fields and counting rules cannot drift.
//! Unlike the legacy local path, every repository error — including the pending-ideation count —
//! propagates to the caller.
//!
//! # Class and events
//!
//! `Read`: the result derives only from DB halt/task/session rows and in-memory registry/atomics;
//! it performs no process inspection and no runtime writes. Reads emit no event.

use std::sync::Arc;

use tauri::State;

use crate::application::AppState;
use crate::commands::execution_commands::{
    compute_execution_status, ActiveProjectState, ExecutionState, ExecutionStatusResponse,
    IdeationWaitingErrorPolicy,
};
use crate::domain::entities::ProjectId;

#[tauri::command]
pub async fn get_remote_execution_status(
    project_id: Option<String>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
) -> Result<ExecutionStatusResponse, String> {
    get_remote_execution_status_for_state(
        project_id,
        execution_state.inner(),
        app_state.inner(),
        active_project_state.inner(),
    )
    .await
}

#[doc(hidden)]
pub async fn get_remote_execution_status_for_state(
    project_id: Option<String>,
    execution_state: &Arc<ExecutionState>,
    app_state: &AppState,
    active_project_state: &Arc<ActiveProjectState>,
) -> Result<ExecutionStatusResponse, String> {
    let effective_project_id = match project_id {
        Some(id) => Some(ProjectId::from_string(id)),
        None => active_project_state.get().await,
    };

    // Deliberate divergence from the legacy local poll: a remote snapshot must fail closed
    // instead of reporting zero pending ideation sessions when that repository read fails.
    compute_execution_status(
        effective_project_id,
        execution_state,
        app_state,
        IdeationWaitingErrorPolicy::FailClosed,
    )
    .await
    .map(|computed| computed.response)
}
