use ralphx_events::{emit_serialized, EventSink};

use crate::shell::event_sink::TauriEventSink;

/// Emit a task lifecycle event (archived, restored, deleted).
///
/// These events share a common payload structure with task and project IDs.
pub fn emit_task_lifecycle_event(
    app: &tauri::AppHandle,
    event_name: &str,
    task_id: &str,
    project_id: &str,
) {
    let events = TauriEventSink::new(app.clone());
    emit_task_lifecycle_event_to_sink(&events, event_name, task_id, project_id);
}

pub fn emit_task_lifecycle_event_to_sink(
    events: &dyn EventSink,
    event_name: &str,
    task_id: &str,
    project_id: &str,
) {
    let _ = emit_serialized(
        events,
        event_name,
        &serde_json::json!({
            "taskId": task_id,
            "projectId": project_id,
        }),
    );
}
