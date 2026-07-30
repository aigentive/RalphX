//! Remote-facade task projections.
//!
//! `get_task_context` is a dual-audience read: local harness agents and the local Tauri
//! frontend consume the FULL `Task` (priority, category, blocked_reason, merge-pipeline and
//! branch metadata, timestamps, raw metadata), while a paired remote device must only ever see
//! the 6-field `WorkerTaskView` allowlist.
//!
//! The narrowing therefore belongs HERE — at the facade that serialises for the wire — not on
//! the shared domain struct. Applying it to `TaskContext::task` itself silently narrowed the
//! local path for every user, including users who never enable `remote_host`.
//!
//! `remote_server::registry` registers this shim as the facade target for `get_task_context`,
//! so the projection is unconditional for remote callers and structurally impossible to bypass:
//! the only remote entry point for that command is this function.

use serde_json::Value;
use tauri::State;

use crate::application::AppState;
use crate::domain::entities::{TaskContext, WorkerTaskView};

/// Facade target for `get_task_context` (see `remote_server::registry`).
///
/// Signature-compatible with `commands::task_context_commands::get_task_context` so the
/// registration shape (`params: [(arg task_id: String), (app_state)]`, `call: async`,
/// `result: fallible`) is unchanged; only the serialised task is narrowed.
pub async fn get_task_context(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let context = crate::commands::task_context_commands::get_task_context(task_id, state).await?;
    project_task_context(context)
}

/// Replaces the full `task` payload with the `WorkerTaskView` allowlist.
///
/// The replacement is a whole-key overwrite (not a merge), so no field of the full `Task` can
/// survive into the remote payload. The allowlist is derived from `WorkerTaskView`'s own
/// serialization rather than restated here, which keeps this seam and
/// `capability_ledger_tests::worker_task_view_allowlist` reading one authority.
pub fn project_task_context(context: TaskContext) -> Result<Value, String> {
    let view = WorkerTaskView::from(context.task.clone());
    let view_value = serde_json::to_value(view)
        .map_err(|error| format!("worker task view could not be serialized: {error}"))?;
    let mut value = serde_json::to_value(context)
        .map_err(|error| format!("task context could not be serialized: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "task context did not serialize to an object".to_string())?;
    object.insert("task".to_string(), view_value);
    Ok(value)
}
