use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tauri::Manager;

use super::notification_commands::{
    dock_badge_label, get_notification_settings, get_unread_notification_count,
    list_attention_items, list_notifications, mark_all_notifications_read, mark_notification_read,
    update_notification_settings, UpdateNotificationSettingsInput,
};
use crate::application::AppState;
use crate::domain::entities::{
    NewNotification, Notification, NotificationCategory, NotificationSettings,
    NotificationSeverity, NotificationTarget,
};
use crate::domain::repositories::{
    NotificationPage, NotificationRepository, NotificationSettingsRepository,
};
use crate::error::{AppError, AppResult};

fn test_app() -> tauri::App<tauri::test::MockRuntime> {
    test_app_with_state(AppState::new_test())
}

fn test_app_with_state(state: AppState) -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

struct FailingNotificationRepository;

#[async_trait]
impl NotificationRepository for FailingNotificationRepository {
    async fn create_with_dedupe(&self, _notification: Notification) -> AppResult<bool> {
        Err(AppError::Database(
            "injected notification repository failure".to_string(),
        ))
    }

    async fn list(
        &self,
        _project_id: Option<&str>,
        _cursor: Option<&str>,
        _limit: u32,
    ) -> AppResult<NotificationPage> {
        Err(AppError::Database(
            "injected notification repository failure".to_string(),
        ))
    }

    async fn unread_count(&self, _project_id: Option<&str>) -> AppResult<u64> {
        Err(AppError::Database(
            "injected notification repository failure".to_string(),
        ))
    }

    async fn mark_read(
        &self,
        _id: &str,
        _read_at: DateTime<Utc>,
    ) -> AppResult<Option<Notification>> {
        Err(AppError::Database(
            "injected notification repository failure".to_string(),
        ))
    }

    async fn mark_read_by_dedupe_key(
        &self,
        _dedupe_key: &str,
        _read_at: DateTime<Utc>,
    ) -> AppResult<Option<Notification>> {
        Err(AppError::Database(
            "injected notification repository failure".to_string(),
        ))
    }

    async fn mark_all_read(
        &self,
        _project_id: Option<&str>,
        _read_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        Err(AppError::Database(
            "injected notification repository failure".to_string(),
        ))
    }

    async fn prune(&self, _read_before: DateTime<Utc>, _max_rows: u32) -> AppResult<()> {
        Err(AppError::Database(
            "injected notification repository failure".to_string(),
        ))
    }
}

struct FailingNotificationSettingsRepository {
    fail_reads: bool,
}

#[async_trait]
impl NotificationSettingsRepository for FailingNotificationSettingsRepository {
    async fn get_settings(&self) -> AppResult<NotificationSettings> {
        if self.fail_reads {
            Err(AppError::Database(
                "injected notification settings read failure".to_string(),
            ))
        } else {
            Ok(NotificationSettings::default())
        }
    }

    async fn update_settings(
        &self,
        _settings: &NotificationSettings,
    ) -> AppResult<NotificationSettings> {
        Err(AppError::Database(
            "injected notification settings write failure".to_string(),
        ))
    }
}

#[tokio::test]
async fn notification_settings_commands_return_defaults_and_persist_partial_updates() {
    let app = test_app();
    let defaults = get_notification_settings(app.state::<AppState>())
        .await
        .expect("defaults should load");
    assert!(defaults.desktop_enabled);
    assert!(defaults.desktop_only_when_unfocused);
    assert!(defaults.focused_toasts_enabled);
    assert!(defaults.desktop_agent_requests_enabled);
    assert!(defaults.desktop_agent_waiting_enabled);
    assert!(defaults.desktop_reviews_enabled);
    assert!(defaults.desktop_task_failures_enabled);
    assert!(defaults.desktop_automation_approvals_enabled);
    assert!(!defaults.desktop_automation_run_completions_enabled);
    assert!(defaults.desktop_git_github_enabled);
    assert!(defaults.muted_project_ids.is_empty());

    let updated = update_notification_settings(
        UpdateNotificationSettingsInput {
            desktop_enabled: Some(false),
            desktop_only_when_unfocused: None,
            focused_toasts_enabled: Some(false),
            desktop_agent_requests_enabled: None,
            desktop_agent_waiting_enabled: None,
            desktop_reviews_enabled: None,
            desktop_task_failures_enabled: None,
            desktop_automation_approvals_enabled: None,
            desktop_automation_run_completions_enabled: Some(true),
            desktop_git_github_enabled: None,
            muted_project_ids: Some(vec!["project-1".to_string()]),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("settings should update");

    assert!(!updated.desktop_enabled);
    assert!(!updated.focused_toasts_enabled);
    assert!(updated.desktop_automation_run_completions_enabled);
    assert!(updated.desktop_reviews_enabled);
    assert_eq!(updated.muted_project_ids, ["project-1"]);
}

#[tokio::test]
async fn notification_settings_command_maps_every_supplied_field_and_preserves_omitted_fields() {
    let app = test_app();
    let updated = update_notification_settings(
        UpdateNotificationSettingsInput {
            desktop_enabled: Some(false),
            desktop_only_when_unfocused: Some(false),
            focused_toasts_enabled: Some(false),
            desktop_agent_requests_enabled: Some(false),
            desktop_agent_waiting_enabled: Some(false),
            desktop_reviews_enabled: Some(false),
            desktop_task_failures_enabled: Some(false),
            desktop_automation_approvals_enabled: Some(false),
            desktop_automation_run_completions_enabled: Some(true),
            desktop_git_github_enabled: Some(false),
            muted_project_ids: Some(vec!["project-a".to_string(), "project-b".to_string()]),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("all settings should update");

    assert!(!updated.desktop_enabled);
    assert!(!updated.desktop_only_when_unfocused);
    assert!(!updated.focused_toasts_enabled);
    assert!(!updated.desktop_agent_requests_enabled);
    assert!(!updated.desktop_agent_waiting_enabled);
    assert!(!updated.desktop_reviews_enabled);
    assert!(!updated.desktop_task_failures_enabled);
    assert!(!updated.desktop_automation_approvals_enabled);
    assert!(updated.desktop_automation_run_completions_enabled);
    assert!(!updated.desktop_git_github_enabled);
    assert_eq!(updated.muted_project_ids, ["project-a", "project-b"]);

    let preserved = update_notification_settings(
        UpdateNotificationSettingsInput {
            desktop_enabled: None,
            desktop_only_when_unfocused: None,
            focused_toasts_enabled: None,
            desktop_agent_requests_enabled: None,
            desktop_agent_waiting_enabled: None,
            desktop_reviews_enabled: None,
            desktop_task_failures_enabled: None,
            desktop_automation_approvals_enabled: None,
            desktop_automation_run_completions_enabled: None,
            desktop_git_github_enabled: None,
            muted_project_ids: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("omitted settings should retain their persisted values");

    assert_eq!(preserved, updated);
}

#[tokio::test]
async fn notification_settings_commands_return_repository_failures_to_the_ipc_caller() {
    let mut read_failure_state = AppState::new_test();
    read_failure_state.notification_settings_repo =
        Arc::new(FailingNotificationSettingsRepository { fail_reads: true });
    let read_failure_app = test_app_with_state(read_failure_state);

    let get_error = get_notification_settings(read_failure_app.state::<AppState>())
        .await
        .expect_err("a settings read failure must reach the IPC caller");
    assert!(get_error.contains("injected notification settings read failure"));

    let update_read_error = update_notification_settings(
        UpdateNotificationSettingsInput {
            desktop_enabled: Some(false),
            desktop_only_when_unfocused: None,
            focused_toasts_enabled: None,
            desktop_agent_requests_enabled: None,
            desktop_agent_waiting_enabled: None,
            desktop_reviews_enabled: None,
            desktop_task_failures_enabled: None,
            desktop_automation_approvals_enabled: None,
            desktop_automation_run_completions_enabled: None,
            desktop_git_github_enabled: None,
            muted_project_ids: None,
        },
        read_failure_app.state::<AppState>(),
    )
    .await
    .expect_err("an update must not continue after its settings read fails");
    assert!(update_read_error.contains("injected notification settings read failure"));

    let mut write_failure_state = AppState::new_test();
    write_failure_state.notification_settings_repo =
        Arc::new(FailingNotificationSettingsRepository { fail_reads: false });
    let write_failure_app = test_app_with_state(write_failure_state);
    let update_write_error = update_notification_settings(
        UpdateNotificationSettingsInput {
            desktop_enabled: Some(false),
            desktop_only_when_unfocused: None,
            focused_toasts_enabled: None,
            desktop_agent_requests_enabled: None,
            desktop_agent_waiting_enabled: None,
            desktop_reviews_enabled: None,
            desktop_task_failures_enabled: None,
            desktop_automation_approvals_enabled: None,
            desktop_automation_run_completions_enabled: None,
            desktop_git_github_enabled: None,
            muted_project_ids: None,
        },
        write_failure_app.state::<AppState>(),
    )
    .await
    .expect_err("a settings write failure must reach the IPC caller");
    assert!(update_write_error.contains("injected notification settings write failure"));
}

fn notification(project_id: &str, title: &str) -> NewNotification {
    NewNotification {
        project_id: Some(project_id.to_string()),
        category: NotificationCategory::ReviewNeeded,
        severity: NotificationSeverity::ActionRequired,
        title: title.to_string(),
        body: Some(format!("{title} body")),
        target: NotificationTarget::none(),
        dedupe_key: Some(format!("{project_id}:{title}")),
    }
}

#[tokio::test]
async fn notification_commands_page_filter_and_mark_only_requested_notifications_read() {
    let app = test_app();
    let state = app.state::<AppState>();
    let notification_service = state.notification_service();
    notification_service
        .record(notification("project-a", "first"))
        .await;
    notification_service
        .record(notification("project-a", "second"))
        .await;
    notification_service
        .record(notification("project-b", "other project"))
        .await;

    let default_page = list_notifications(None, None, None, app.state::<AppState>())
        .await
        .expect("default notification page should load");
    assert_eq!(default_page.notifications.len(), 3);
    assert!(!default_page.has_more);

    assert_eq!(
        get_unread_notification_count(None, app.state::<AppState>())
            .await
            .expect("global unread count should load"),
        3
    );
    assert_eq!(
        get_unread_notification_count(Some("project-a".to_string()), app.state::<AppState>())
            .await
            .expect("project unread count should load"),
        2
    );

    let first_page = list_notifications(
        Some("project-a".to_string()),
        None,
        Some(1),
        app.state::<AppState>(),
    )
    .await
    .expect("first page should load");
    assert_eq!(first_page.notifications.len(), 1);
    assert!(first_page.has_more);
    let cursor = first_page
        .cursor
        .expect("first page should provide a cursor");
    let first_id = first_page.notifications[0].id.clone();

    let second_page = list_notifications(
        Some("project-a".to_string()),
        Some(cursor),
        Some(1),
        app.state::<AppState>(),
    )
    .await
    .expect("cursor page should load");
    assert_eq!(second_page.notifications.len(), 1);
    assert_ne!(second_page.notifications[0].id, first_id);
    assert!(!second_page.has_more);

    mark_notification_read("missing-notification".to_string(), app.state::<AppState>())
        .await
        .expect("an already-absent notification is a harmless no-op");
    assert_eq!(
        get_unread_notification_count(Some("project-a".to_string()), app.state::<AppState>())
            .await
            .expect("missing read must not change unread rows"),
        2
    );

    mark_notification_read(first_id.clone(), app.state::<AppState>())
        .await
        .expect("single notification should be marked read");
    assert_eq!(
        get_unread_notification_count(Some("project-a".to_string()), app.state::<AppState>())
            .await
            .expect("read count should refresh"),
        1
    );

    mark_all_notifications_read(Some("project-a".to_string()), app.state::<AppState>())
        .await
        .expect("project notifications should be marked read");
    assert_eq!(
        get_unread_notification_count(Some("project-a".to_string()), app.state::<AppState>())
            .await
            .expect("project unread count should be zero"),
        0
    );
    assert_eq!(
        get_unread_notification_count(Some("project-b".to_string()), app.state::<AppState>())
            .await
            .expect("other project must remain unread"),
        1
    );

    mark_all_notifications_read(None, app.state::<AppState>())
        .await
        .expect("global mark-read should clear the remaining notification");
    assert_eq!(
        get_unread_notification_count(None, app.state::<AppState>())
            .await
            .expect("global unread count should be zero"),
        0
    );
}

#[tokio::test]
async fn notification_read_commands_return_repository_failures_to_the_ipc_caller() {
    let mut state = AppState::new_test();
    state.notification_repo = Arc::new(FailingNotificationRepository);
    let app = test_app_with_state(state);

    let list_error = list_notifications(None, None, None, app.state::<AppState>())
        .await
        .expect_err("notification list failure must reach the IPC caller");
    assert!(list_error.contains("injected notification repository failure"));

    let count_error = get_unread_notification_count(None, app.state::<AppState>())
        .await
        .expect_err("notification count failure must reach the IPC caller");
    assert!(count_error.contains("injected notification repository failure"));

    let read_error = mark_notification_read("notification-1".to_string(), app.state::<AppState>())
        .await
        .expect_err("notification read failure must reach the IPC caller");
    assert!(read_error.contains("injected notification repository failure"));

    let mark_all_error = mark_all_notifications_read(None, app.state::<AppState>())
        .await
        .expect_err("mark-all-read failure must reach the IPC caller");
    assert!(mark_all_error.contains("injected notification repository failure"));
}

#[tokio::test]
async fn list_attention_items_command_returns_an_empty_actionable_list_without_live_work() {
    let app = test_app();

    assert!(list_attention_items(None, app.state::<AppState>())
        .await
        .expect("global attention read should succeed")
        .is_empty());
    assert!(
        list_attention_items(Some("project-a".to_string()), app.state::<AppState>())
            .await
            .expect("project attention read should succeed")
            .is_empty()
    );
}

#[test]
fn notification_settings_input_ignores_unknown_json_fields() {
    let input: UpdateNotificationSettingsInput =
        serde_json::from_str(r#"{"desktopEnabled":false,"futurePreference":true}"#)
            .expect("unknown fields should be ignored");

    assert_eq!(input.desktop_enabled, Some(false));
    assert_eq!(input.focused_toasts_enabled, None);
    assert_eq!(input.muted_project_ids, None);
}

#[test]
fn dock_badge_label_uses_the_full_attention_count_and_clears_at_zero() {
    assert_eq!(dock_badge_label(7), Some("7".to_owned()));
    assert_eq!(dock_badge_label(10), Some("10".to_owned()));
    assert_eq!(dock_badge_label(0), None);
}
