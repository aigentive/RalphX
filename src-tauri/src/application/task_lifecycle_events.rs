use tauri::Emitter;

/// Emit a task lifecycle event (archived, restored, deleted).
///
/// These events share a common payload structure with task and project IDs.
pub fn emit_task_lifecycle_event(
    app: &tauri::AppHandle,
    event_name: &str,
    task_id: &str,
    project_id: &str,
) {
    let _ = app.emit(
        event_name,
        serde_json::json!({
            "taskId": task_id,
            "projectId": project_id,
        }),
    );
}
