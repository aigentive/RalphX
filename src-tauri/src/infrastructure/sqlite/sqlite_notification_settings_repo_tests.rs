use super::*;
use crate::domain::repositories::NotificationSettingsRepository;
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn notification_settings_defaults_apply_to_an_empty_database() {
    let db = SqliteTestDb::new("notification-settings-defaults");
    let repo = SqliteNotificationSettingsRepository::from_shared(db.shared_conn());

    let settings = repo.get_settings().await.unwrap();
    assert!(settings.desktop_enabled);
    assert!(settings.desktop_only_when_unfocused);
    assert!(settings.focused_toasts_enabled);
    assert!(settings.desktop_agent_requests_enabled);
    assert!(settings.desktop_agent_waiting_enabled);
    assert!(settings.desktop_reviews_enabled);
    assert!(settings.desktop_task_failures_enabled);
    assert!(settings.desktop_automation_approvals_enabled);
    assert!(!settings.desktop_automation_run_completions_enabled);
    assert!(settings.desktop_git_github_enabled);
    assert!(settings.muted_project_ids.is_empty());
}

#[tokio::test]
async fn notification_settings_update_persists_every_field() {
    let db = SqliteTestDb::new("notification-settings-update");
    let repo = SqliteNotificationSettingsRepository::from_shared(db.shared_conn());
    let settings = NotificationSettings {
        desktop_enabled: false,
        desktop_only_when_unfocused: false,
        focused_toasts_enabled: false,
        desktop_agent_requests_enabled: false,
        desktop_agent_waiting_enabled: false,
        desktop_reviews_enabled: false,
        desktop_task_failures_enabled: false,
        desktop_automation_approvals_enabled: false,
        desktop_automation_run_completions_enabled: true,
        desktop_git_github_enabled: false,
        muted_project_ids: vec!["project-1".to_string(), "project-2".to_string()],
    };

    assert_eq!(repo.update_settings(&settings).await.unwrap(), settings);
    assert_eq!(repo.get_settings().await.unwrap(), settings);
}

#[tokio::test]
async fn notification_settings_ignore_unknown_json_fields() {
    let db = SqliteTestDb::new("notification-settings-extra-json");
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO notification_settings (id, settings_json) VALUES (1, ?1)",
            [r#"{"desktop_enabled":false,"future_toggle":true}"#],
        )
        .unwrap();
    });
    let repo = SqliteNotificationSettingsRepository::from_shared(db.shared_conn());

    let settings = repo.get_settings().await.unwrap();
    assert!(!settings.desktop_enabled);
    assert!(settings.desktop_only_when_unfocused);
    assert!(!settings.desktop_automation_run_completions_enabled);
    assert!(settings.muted_project_ids.is_empty());
}
