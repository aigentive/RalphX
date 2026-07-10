use tauri::State;

use crate::application::attention_service::AttentionService;
use crate::domain::entities::AttentionItem;
use crate::domain::repositories::NotificationPage;
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

#[tauri::command]
pub async fn list_notifications(
    project_id: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<NotificationPage, String> {
    state
        .notification_repo
        .list(
            project_id.as_deref(),
            cursor.as_deref(),
            limit.unwrap_or(50),
        )
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn mark_notification_read(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.notification_service().mark_read(&id).await;
    Ok(())
}

#[tauri::command]
pub async fn mark_all_notifications_read(
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .notification_service()
        .mark_all_read(project_id.as_deref())
        .await;
    Ok(())
}

#[tauri::command]
pub async fn get_unread_notification_count(
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u64, String> {
    state
        .notification_repo
        .unread_count(project_id.as_deref())
        .await
        .map_err(|error| error.to_string())
}
