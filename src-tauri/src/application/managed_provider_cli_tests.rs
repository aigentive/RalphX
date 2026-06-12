use crate::domain::agents::{
    AgentHarnessKind, AgentProviderCliManagementMode, AgentProviderSettings,
};
use crate::utils::runtime_log_paths::managed_codex_binary_path;

use super::{managed_provider_cli_launch_path, managed_provider_runtime_probe};

fn provider_settings(
    provider: AgentHarnessKind,
    mode: AgentProviderCliManagementMode,
) -> AgentProviderSettings {
    let mut settings = AgentProviderSettings::disabled_defaults(provider);
    settings.cli_management_mode = mode;
    settings
}

#[test]
fn user_managed_provider_has_no_managed_launch_override() {
    let settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::UserManaged,
    );

    assert!(managed_provider_cli_launch_path(&settings).is_none());
    assert!(managed_provider_runtime_probe(&settings).is_none());
}

#[test]
fn rx_managed_codex_launches_from_app_owned_binary_path() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::RxManaged,
    );

    let path = managed_provider_cli_launch_path(&settings)
        .expect("managed Codex path override")
        .expect("managed Codex path");

    assert_eq!(path, managed_codex_binary_path());
}

#[test]
fn rx_managed_native_claude_uses_default_launch_resolution() {
    let settings = provider_settings(
        AgentHarnessKind::Claude,
        AgentProviderCliManagementMode::RxManaged,
    );

    assert!(managed_provider_cli_launch_path(&settings).is_none());
    assert!(managed_provider_runtime_probe(&settings).is_none());
}
