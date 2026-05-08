use ralphx_lib::application::AppState;
use ralphx_lib::commands::harness_provider_commands::{
    get_agent_provider_settings, update_agent_provider_settings, UpdateAgentProviderSettingsInput,
};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

fn provider_command_app() -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(AppState::new_test())
        .build(mock_context(noop_assets()))
        .expect("mock app should build")
}

fn input(provider: &str) -> UpdateAgentProviderSettingsInput {
    UpdateAgentProviderSettingsInput {
        provider: provider.to_string(),
        enabled: None,
        is_default: None,
        model: None,
        effort: None,
        approval_policy: None,
        sandbox_mode: None,
        claude_permission_mode: None,
        claude_dangerously_skip_permissions: None,
        claude_allow_dangerously_skip_permissions: None,
        apply_to_all_lanes: false,
    }
}

#[tokio::test]
async fn ipc_contract_provider_settings_read_returns_all_known_providers() {
    let app = provider_command_app();

    let response = get_agent_provider_settings(app.state::<AppState>())
        .await
        .expect("provider settings should load");

    assert_eq!(response.providers.len(), 2);
    assert_eq!(response.providers[0].provider, "codex");
    assert_eq!(response.providers[1].provider, "claude");
}

#[tokio::test]
async fn ipc_contract_provider_settings_update_round_trips_provider_defaults() {
    let app = provider_command_app();

    let updated = update_agent_provider_settings(
        UpdateAgentProviderSettingsInput {
            model: Some("gpt-5.4".to_string()),
            effort: Some("high".to_string()),
            approval_policy: Some("never".to_string()),
            sandbox_mode: Some("danger-full-access".to_string()),
            ..input("codex")
        },
        app.state::<AppState>(),
    )
    .await
    .expect("provider settings should update");

    let codex = updated
        .providers
        .iter()
        .find(|provider| provider.provider == "codex")
        .expect("codex provider response");
    assert_eq!(codex.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(codex.effort.as_deref(), Some("high"));
    assert_eq!(codex.approval_policy.as_deref(), Some("never"));
    assert_eq!(codex.sandbox_mode.as_deref(), Some("danger-full-access"));

    let read_back = get_agent_provider_settings(app.state::<AppState>())
        .await
        .expect("provider settings should read back");
    let read_back_codex = read_back
        .providers
        .iter()
        .find(|provider| provider.provider == "codex")
        .expect("codex provider response");
    assert_eq!(read_back_codex.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(read_back_codex.effort.as_deref(), Some("high"));
}
