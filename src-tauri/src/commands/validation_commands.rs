use tauri::State;

use crate::application::{AppState, TaskValidationService, TaskValidationSummary};
use crate::domain::entities::TaskId;

#[tauri::command]
pub async fn get_task_validation_summary(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<TaskValidationSummary, String> {
    let task_id = TaskId::from_string(task_id);
    TaskValidationService::get_task_validation_summary(&state, &task_id)
        .await
        .map_err(|e| e.to_string())
}
