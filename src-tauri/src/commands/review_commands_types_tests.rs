use super::review_commands_types::ReviewSettingsResponse;
use crate::domain::review::ReviewSettings;

#[test]
fn review_settings_response_includes_task_validation_policy() {
    let settings = ReviewSettings {
        run_task_validations: false,
        autofix_workspace_review_blocking_findings: false,
        workspace_review_fixer_cycle_cap: 0,
        ..ReviewSettings::default()
    };

    let response = ReviewSettingsResponse::from(settings);

    assert!(!response.run_task_validations);
    assert!(!response.autofix_workspace_review_blocking_findings);
    assert_eq!(response.workspace_review_fixer_cycle_cap, 0);
    assert!(response.ai_review_enabled);
    assert!(!response.auto_create_followup_agent_conversation);
}
