use super::*;
use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::entities::UiFeatureFlagOverrides;
use crate::domain::repositories::UiFeatureFlagOverridesRepository;
use crate::error::{AppError, AppResult};

struct FailingCapabilityOverridesRepository;

#[async_trait]
impl UiFeatureFlagOverridesRepository for FailingCapabilityOverridesRepository {
    async fn get(&self) -> AppResult<UiFeatureFlagOverrides> {
        Ok(UiFeatureFlagOverrides::default())
    }

    async fn set_agent_personas(&self, _value: Option<bool>) -> AppResult<()> {
        Ok(())
    }

    async fn update_agent_capabilities(
        &self,
        _team: Option<bool>,
        _workflows: Option<bool>,
        _autopilot: Option<bool>,
    ) -> AppResult<UiFeatureFlagOverrides> {
        Err(AppError::Database("simulated write failure".to_string()))
    }
}

#[test]
fn get_ui_feature_flags_includes_agent_personas() {
    let state = AppState::new_test();
    let response = ui_feature_flags_response(&state);
    let json = serde_json::to_value(response).expect("feature flags response should serialize");

    assert_eq!(json.get("agentPersonas"), Some(&serde_json::json!(false)));
    assert!(json.get("composerFolderReferences").is_none());
    assert!(matches!(
        json.get("standaloneConversations"),
        Some(serde_json::Value::Bool(_))
    ));
    assert_eq!(
        json.get("agentConversationTeam"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(
        json.get("agentConversationWorkflows"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(
        json.get("agentConversationAutopilot"),
        Some(&serde_json::json!(false))
    );
    assert!(json.get("agent_personas").is_none());
}

#[test]
fn get_ui_feature_flags_ships_remote_environments_visible_by_default() {
    let state = AppState::new_test();
    let response = ui_feature_flags_response(&state);
    let json = serde_json::to_value(response).expect("feature flags response should serialize");

    assert_eq!(
        json.get("remoteEnvironments"),
        Some(&serde_json::json!(true)),
        "remoteEnvironments defaults ON (owner decision, 2026-08-03) — the remote settings \
         surfaces ship visible; config/env can still disable them"
    );
    assert!(json.get("remote_environments").is_none());
}

#[test]
fn get_ui_feature_flags_reports_the_effective_standalone_value() {
    let state = AppState::new_test();

    assert!(ui_feature_flags_response_with_standalone(&state, true).standalone_conversations);
    assert!(!ui_feature_flags_response_with_standalone(&state, false).standalone_conversations);
}

#[test]
fn test_ui_feature_flags_response_serializes_to_camel_case() {
    let state = AppState::new_test();
    let response = ui_feature_flags_response(&state);
    let json = serde_json::to_string(&response).unwrap();
    // Verify camelCase field names (serde rename_all = "camelCase")
    assert!(
        json.contains("\"activityPage\":"),
        "Expected camelCase 'activityPage' in JSON: {json}"
    );
    assert!(
        json.contains("\"extensibilityPage\":"),
        "Expected camelCase 'extensibilityPage' in JSON: {json}"
    );
    assert!(!json.contains("\"ideationPage\":"));
    assert!(!json.contains("\"battleMode\":"));
    assert!(
        json.contains("\"automationsPage\":"),
        "Expected camelCase 'automationsPage' in JSON: {json}"
    );
    assert!(
        json.contains("\"atlassianOauth\":"),
        "Expected camelCase 'atlassianOauth' in JSON: {json}"
    );
    assert!(
        json.contains("\"ticketingDashboard\":"),
        "Expected camelCase 'ticketingDashboard' in JSON: {json}"
    );
    // Verify snake_case is NOT present
    assert!(
        !json.contains("\"activity_page\":"),
        "Unexpected snake_case 'activity_page' in JSON: {json}"
    );
    assert!(
        !json.contains("\"extensibility_page\":"),
        "Unexpected snake_case 'extensibility_page' in JSON: {json}"
    );
    assert!(
        !json.contains("\"automations_page\":"),
        "Unexpected snake_case 'automations_page' in JSON: {json}"
    );
    assert!(
        !json.contains("\"atlassian_oauth\":"),
        "Unexpected snake_case 'atlassian_oauth' in JSON: {json}"
    );
    assert!(
        !json.contains("\"ticketing_dashboard\":"),
        "Unexpected snake_case 'ticketing_dashboard' in JSON: {json}"
    );
}

#[tokio::test]
async fn updating_capabilities_persists_then_updates_the_live_gate() {
    let state = AppState::new_test();

    let response = update_ui_feature_flags_for_state(
        UpdateUiFeatureFlagsInput {
            agent_personas: None,
            agent_conversation_team: Some(true),
            agent_conversation_workflows: Some(false),
            agent_conversation_autopilot: Some(true),
        },
        &state,
    )
    .await
    .expect("capability update should succeed");

    let persisted = state
        .ui_feature_flag_overrides_repo
        .get()
        .await
        .expect("capability row should be readable");
    assert!(persisted.agent_conversation_team);
    assert!(!persisted.agent_conversation_workflows);
    assert!(persisted.agent_conversation_autopilot);
    assert!(state.agent_capability_gate.team_enabled());
    assert!(!state.agent_capability_gate.workflows_enabled());
    assert!(state.agent_capability_gate.autopilot_enabled());
    assert!(response.agent_conversation_team);
    assert!(!response.agent_conversation_workflows);
    assert!(response.agent_conversation_autopilot);
}

#[tokio::test]
async fn failed_capability_write_leaves_the_live_gate_unchanged() {
    let mut state = AppState::new_test();
    state.ui_feature_flag_overrides_repo = Arc::new(FailingCapabilityOverridesRepository);

    let error = update_ui_feature_flags_for_state(
        UpdateUiFeatureFlagsInput {
            agent_personas: None,
            agent_conversation_team: Some(true),
            agent_conversation_workflows: None,
            agent_conversation_autopilot: None,
        },
        &state,
    )
    .await
    .expect_err("failed persistence must reject the settings update");

    assert!(error.contains("simulated write failure"));
    assert!(!state.agent_capability_gate.team_enabled());
    assert!(!state.agent_capability_gate.workflows_enabled());
}
