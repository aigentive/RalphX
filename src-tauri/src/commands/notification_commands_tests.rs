use tauri::Manager;

use super::notification_commands::{
    get_notification_settings, update_notification_settings, UpdateNotificationSettingsInput,
};
use crate::application::AppState;

fn test_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
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
        },
        app.state::<AppState>(),
    )
    .await
    .expect("settings should update");

    assert!(!updated.desktop_enabled);
    assert!(!updated.focused_toasts_enabled);
    assert!(updated.desktop_automation_run_completions_enabled);
    assert!(updated.desktop_reviews_enabled);
}

#[test]
fn notification_settings_input_ignores_unknown_json_fields() {
    let input: UpdateNotificationSettingsInput =
        serde_json::from_str(r#"{"desktopEnabled":false,"futurePreference":true}"#)
            .expect("unknown fields should be ignored");

    assert_eq!(input.desktop_enabled, Some(false));
    assert_eq!(input.focused_toasts_enabled, None);
}
