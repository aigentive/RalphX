use tauri::State;

use crate::application::attention_service::AttentionService;
use crate::domain::entities::AttentionItem;
use crate::domain::entities::NotificationSettings;
use crate::domain::repositories::NotificationPage;
use crate::AppState;

pub(crate) fn dock_badge_label(count: u32) -> Option<String> {
    (count > 0).then(|| count.to_string())
}

#[cfg(target_os = "macos")]
fn set_macos_dock_badge(label: Option<String>) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let app = NSApplication::sharedApplication(mtm);
    let dock_tile = app.dockTile();
    let label = label.as_deref().map(NSString::from_str);
    dock_tile.setBadgeLabel(label.as_deref());
}

/// Mirrors the frontend-owned attention count to the macOS Dock badge.
#[tauri::command]
pub fn set_dock_badge_count(count: u32, app: tauri::AppHandle) -> Result<(), String> {
    let label = dock_badge_label(count);

    #[cfg(target_os = "macos")]
    app.run_on_main_thread(move || set_macos_dock_badge(label))
        .map_err(|error| error.to_string())?;

    #[cfg(not(target_os = "macos"))]
    let _ = (app, label);

    Ok(())
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNotificationSettingsInput {
    pub desktop_enabled: Option<bool>,
    pub desktop_only_when_unfocused: Option<bool>,
    pub focused_toasts_enabled: Option<bool>,
    pub desktop_agent_requests_enabled: Option<bool>,
    pub desktop_agent_waiting_enabled: Option<bool>,
    pub desktop_reviews_enabled: Option<bool>,
    pub desktop_task_failures_enabled: Option<bool>,
    pub desktop_automation_approvals_enabled: Option<bool>,
    pub desktop_automation_run_completions_enabled: Option<bool>,
    pub desktop_git_github_enabled: Option<bool>,
    pub muted_project_ids: Option<Vec<String>>,
}

/// Gets the persisted global notification preferences, or product defaults before first save.
#[tauri::command]
pub async fn get_notification_settings(
    state: State<'_, AppState>,
) -> Result<NotificationSettings, String> {
    state
        .notification_settings_repo
        .get_settings()
        .await
        .map_err(|error| error.to_string())
}

/// Updates only supplied notification preferences so future fields remain forward-compatible.
#[tauri::command]
pub async fn update_notification_settings(
    input: UpdateNotificationSettingsInput,
    state: State<'_, AppState>,
) -> Result<NotificationSettings, String> {
    let mut settings = state
        .notification_settings_repo
        .get_settings()
        .await
        .map_err(|error| error.to_string())?;

    if let Some(value) = input.desktop_enabled {
        settings.desktop_enabled = value;
    }
    if let Some(value) = input.desktop_only_when_unfocused {
        settings.desktop_only_when_unfocused = value;
    }
    if let Some(value) = input.focused_toasts_enabled {
        settings.focused_toasts_enabled = value;
    }
    if let Some(value) = input.desktop_agent_requests_enabled {
        settings.desktop_agent_requests_enabled = value;
    }
    if let Some(value) = input.desktop_agent_waiting_enabled {
        settings.desktop_agent_waiting_enabled = value;
    }
    if let Some(value) = input.desktop_reviews_enabled {
        settings.desktop_reviews_enabled = value;
    }
    if let Some(value) = input.desktop_task_failures_enabled {
        settings.desktop_task_failures_enabled = value;
    }
    if let Some(value) = input.desktop_automation_approvals_enabled {
        settings.desktop_automation_approvals_enabled = value;
    }
    if let Some(value) = input.desktop_automation_run_completions_enabled {
        settings.desktop_automation_run_completions_enabled = value;
    }
    if let Some(value) = input.desktop_git_github_enabled {
        settings.desktop_git_github_enabled = value;
    }
    if let Some(value) = input.muted_project_ids {
        settings.muted_project_ids = value;
    }

    state
        .notification_settings_repo
        .update_settings(&settings)
        .await
        .map_err(|error| error.to_string())
}

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
    state
        .notification_service()
        .mark_read(&id)
        .await
        .map_err(|error| error.to_string())?;
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
        .await
        .map_err(|error| error.to_string())?;
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

/// Requests notification permission when needed, then sends a development smoke notification.
///
/// This command is intentionally unavailable from release builds. Desktop notification dispatch
/// remains owned by `NotificationService` when that integration is added.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn debug_send_test_notification(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_notification::{NotificationExt, PermissionState};

    let notification = app.notification();
    if notification
        .permission_state()
        .map_err(|error| error.to_string())?
        == PermissionState::Prompt
    {
        notification
            .request_permission()
            .map_err(|error| error.to_string())?;
    }

    if notification
        .permission_state()
        .map_err(|error| error.to_string())?
        != PermissionState::Granted
    {
        return Err("Desktop notification permission was not granted".to_string());
    }

    notification
        .builder()
        .title("RalphX notification smoke test")
        .body("Desktop notifications are available in this development build.")
        .show()
        .map_err(|error| error.to_string())
}
