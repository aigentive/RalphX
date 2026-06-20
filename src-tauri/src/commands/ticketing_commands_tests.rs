use super::*;
use crate::application::LinearIntegrationSettings;
use crate::domain::integrations::{AtlassianIntegrationSettings, IntegrationValidationStatus};

#[test]
fn provider_summaries_reflect_existing_integration_settings() {
    let jira = AtlassianIntegrationSettings {
        enabled: true,
        validation_status: IntegrationValidationStatus::Valid,
        jira_available: true,
        ..Default::default()
    };

    let linear = LinearIntegrationSettings {
        enabled: true,
        validation_status: IntegrationValidationStatus::Invalid,
        last_error: Some("Token rejected".to_string()),
        ..Default::default()
    };

    let jira_summary = jira_provider_summary(&jira);
    let linear_summary = linear_provider_summary(&linear);

    assert_eq!(jira_summary.provider, "jira");
    assert_eq!(jira_summary.connection_status, "connected");
    assert!(jira_summary.capabilities.supports_kanban);
    assert_eq!(linear_summary.provider, "linear");
    assert_eq!(linear_summary.connection_status, "error");
    assert_eq!(
        linear_summary.error_message.as_deref(),
        Some("Token rejected")
    );
}

#[test]
fn ticketing_columns_return_provider_neutral_defaults() {
    let columns = default_ticketing_columns();

    assert_eq!(columns.len(), 4);
    assert_eq!(columns[0].id, "todo");
    assert_eq!(columns[1].category, "in_progress");
    assert_eq!(columns[2].category, "done");
}

#[test]
fn provider_validation_rejects_unknown_ticketing_provider() {
    let error = validate_provider("github").expect_err("unknown provider should fail");

    assert!(error.contains("Unknown ticketing provider"));
}
