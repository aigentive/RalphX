use tauri::State;

use crate::application::attention_service::AttentionService;
use crate::domain::entities::AttentionItem;
use crate::AppState;

/// Lists live, human-actionable attention items for the notification center.
///
/// Repository reads are fail-closed: if any authoritative source cannot be loaded, this command
/// returns an error rather than treating a partial result as complete. Results are grouped by
/// urgency (agent requests, reviews, tasks, automations, git) and newest first within each group.
#[tauri::command]
pub async fn list_attention_items(
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AttentionItem>, String> {
    AttentionService::from_app_state(&state)
        .list_attention_items(project_id.as_deref())
        .await
        .map_err(|error| error.to_string())
}
