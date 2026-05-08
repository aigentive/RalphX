use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};

use super::{merge_input, UpdateAgentProviderSettingsInput};

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

#[test]
fn merge_rejects_enable_when_provider_is_not_available() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    let next = UpdateAgentProviderSettingsInput {
        enabled: Some(true),
        ..input("codex")
    };

    let err = merge_input(settings, next, false).expect_err("enable should fail");

    assert!(err.contains("cannot be enabled"));
}

#[test]
fn merge_rejects_default_when_provider_is_not_enabled() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    let next = UpdateAgentProviderSettingsInput {
        is_default: Some(true),
        ..input("codex")
    };

    let err = merge_input(settings, next, true).expect_err("default should fail");

    assert!(err.contains("cannot be the default"));
}

#[test]
fn merge_accepts_enabled_default_provider() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    let next = UpdateAgentProviderSettingsInput {
        enabled: Some(true),
        is_default: Some(true),
        ..input("codex")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert!(merged.enabled);
    assert!(merged.is_default);
}
