use super::review_commands::{
    ensure_human_review_followup_status, ensure_re_review_from_escalated_status,
    update_review_settings, UpdateReviewSettingsInput,
};
use crate::application::AppState;
use crate::domain::entities::InternalStatus;
use tauri::Manager;

#[test]
fn human_review_followup_accepts_review_passed_and_escalated() {
    assert!(ensure_human_review_followup_status(InternalStatus::ReviewPassed, "approve").is_ok());
    assert!(
        ensure_human_review_followup_status(InternalStatus::Escalated, "request changes").is_ok()
    );
}

#[test]
fn human_review_followup_rejects_terminal_statuses() {
    let error = ensure_human_review_followup_status(InternalStatus::Merged, "approve")
        .expect_err("merged task must be rejected");
    assert!(error.contains("review_passed"));
    assert!(error.contains("escalated"));
    assert!(error.contains("merged"));
}

#[test]
fn rereview_requires_escalated_status() {
    assert!(ensure_re_review_from_escalated_status(InternalStatus::Escalated).is_ok());

    let error = ensure_re_review_from_escalated_status(InternalStatus::ReviewPassed)
        .expect_err("review_passed task must be rejected");
    assert!(error.contains("escalated"));
    assert!(error.contains("review_passed"));
}

#[tokio::test]
async fn update_review_settings_toggles_task_validation_policy() {
    let app = tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");

    let response = update_review_settings(
        UpdateReviewSettingsInput {
            require_human_review: None,
            require_workspace_review: None,
            max_fix_attempts: None,
            max_revision_cycles: None,
            auto_create_followup_agent_conversation: None,
            autofix_workspace_review_blocking_findings: None,
            workspace_review_fixer_cycle_cap: None,
            run_task_validations: Some(false),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("review settings update should succeed");

    assert!(!response.run_task_validations);
    let settings = app
        .state::<AppState>()
        .review_settings_repo
        .get_settings()
        .await
        .expect("settings should be persisted");
    assert!(!settings.run_task_validations);
}

#[tokio::test]
async fn update_review_settings_toggles_workspace_review_autofix() {
    let app = tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");

    let response = update_review_settings(
        UpdateReviewSettingsInput {
            require_human_review: None,
            require_workspace_review: None,
            max_fix_attempts: None,
            max_revision_cycles: None,
            auto_create_followup_agent_conversation: None,
            autofix_workspace_review_blocking_findings: Some(false),
            workspace_review_fixer_cycle_cap: None,
            run_task_validations: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("review settings update should succeed");

    assert!(!response.autofix_workspace_review_blocking_findings);
    let settings = app
        .state::<AppState>()
        .review_settings_repo
        .get_settings()
        .await
        .expect("settings should be persisted");
    assert!(!settings.autofix_workspace_review_blocking_findings);
}

#[tokio::test]
async fn update_review_settings_clamps_workspace_review_fixer_cycle_cap() {
    let app = tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");

    let response = update_review_settings(
        UpdateReviewSettingsInput {
            require_human_review: None,
            require_workspace_review: None,
            max_fix_attempts: None,
            max_revision_cycles: None,
            auto_create_followup_agent_conversation: None,
            autofix_workspace_review_blocking_findings: None,
            workspace_review_fixer_cycle_cap: Some(-4),
            run_task_validations: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("review settings update should succeed");

    assert_eq!(response.workspace_review_fixer_cycle_cap, 0);
    let settings = app
        .state::<AppState>()
        .review_settings_repo
        .get_settings()
        .await
        .expect("settings should be persisted");
    assert_eq!(settings.workspace_review_fixer_cycle_cap, 0);
}
