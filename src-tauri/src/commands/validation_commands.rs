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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{Project, Task};
    use tauri::Manager;

    #[tokio::test]
    async fn get_task_validation_summary_command_returns_default_summary() {
        let state = AppState::new_test();
        let temp_dir = tempfile::tempdir().expect("temp project dir");
        let project = state
            .project_repo
            .create(Project::new(
                "Validation Command".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
            ))
            .await
            .expect("project should be created");
        let task = Task::new(project.id.clone(), "Summarize validation".to_string());
        let task_id = task.id.clone();
        state
            .task_repo
            .create(task)
            .await
            .expect("task should be created");
        let app = tauri::test::mock_builder()
            .manage(state)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app should build");

        let summary =
            get_task_validation_summary(task_id.as_str().to_string(), app.state::<AppState>())
                .await
                .expect("summary command should succeed");

        assert_eq!(summary.task_id, task_id.as_str());
        assert_eq!(summary.project_id, project.id.as_str());
        assert!(summary.policy_enabled);
        assert!(summary.latest_run.is_none());
        assert!(summary.commands.is_empty());
        assert!(summary.disabled_reason.is_none());
    }
}
