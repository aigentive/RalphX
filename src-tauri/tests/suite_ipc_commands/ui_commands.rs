use ralphx_lib::application::AppState;
use ralphx_lib::commands::ui_commands::{
    get_ui_feature_flags, update_ui_feature_flags, UpdateUiFeatureFlagsInput,
};
use ralphx_lib::infrastructure::agents::claude::ui_feature_flags_config;
use ralphx_lib::infrastructure::agents::{
    agent_personas_enabled, reset_agent_personas_override_for_test, set_agent_personas_override,
};
use tauri::Manager;

struct PersonaFlagOverrideReset;

impl Drop for PersonaFlagOverrideReset {
    fn drop(&mut self) {
        reset_agent_personas_override_for_test();
    }
}

fn persona_flag_override_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("feature flag command mock app should build")
}

#[tokio::test]
async fn persona_flag_override_update_persists_and_updates_effective_response() {
    let _reset = PersonaFlagOverrideReset;
    reset_agent_personas_override_for_test();
    let app = persona_flag_override_command_app();

    let response = update_ui_feature_flags(
        UpdateUiFeatureFlagsInput {
            agent_personas: Some(true),
            agent_conversation_team: None,
            agent_conversation_workflows: None,
            agent_conversation_autopilot: None,
        },
        app.state(),
    )
    .await
    .expect("feature flag update should succeed");

    assert!(response.agent_personas);
    assert!(agent_personas_enabled());
    assert_eq!(
        app.state::<AppState>()
            .ui_feature_flag_overrides_repo
            .get()
            .await
            .expect("persisted override read should succeed")
            .agent_personas,
        Some(true)
    );
}

#[test]
fn persona_flag_override_get_reports_live_effective_value() {
    let _reset = PersonaFlagOverrideReset;
    set_agent_personas_override(Some(true));
    let app = persona_flag_override_command_app();

    assert!(get_ui_feature_flags(app.state()).agent_personas);
}

#[test]
fn persona_flag_override_precedence_uses_override_then_config() {
    let _reset = PersonaFlagOverrideReset;

    set_agent_personas_override(Some(false));
    assert!(!agent_personas_enabled());

    set_agent_personas_override(None);
    assert_eq!(
        agent_personas_enabled(),
        ui_feature_flags_config().agent_personas,
        "an unset database override must fall back to the cached env/yaml configuration"
    );
}
