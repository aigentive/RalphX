use crate::application::{harness_runtime_registry::HarnessRuntimeProbe, AppState, AGENT_LANES};
use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentProviderSettings, LogicalEffort,
    CODEX_DEFAULT_APPROVAL_POLICY, CODEX_DEFAULT_SANDBOX_MODE,
};
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;
use std::collections::HashMap;
use std::sync::Arc;

use super::{
    apply_provider_to_global_lanes, merge_input, parse_effort, parse_provider, provider_status,
    read_provider_settings, read_provider_settings_with_probes, to_lane_settings, to_response,
    update_provider_settings_with_probes, UpdateAgentProviderSettingsInput,
};

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
        reset_to_defaults: false,
        apply_to_all_lanes: false,
    }
}

fn ready_probe(path: &str) -> HarnessRuntimeProbe {
    HarnessRuntimeProbe {
        binary_path: Some(path.to_string()),
        binary_found: true,
        probe_succeeded: true,
        available: true,
        missing_core_exec_features: Vec::new(),
        error: None,
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

#[test]
fn merge_applies_provider_defaults_and_clears_blank_fields() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    let next = UpdateAgentProviderSettingsInput {
        model: Some(" ".to_string()),
        effort: Some("xhigh".to_string()),
        approval_policy: Some(" ".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
        claude_permission_mode: Some("bypassPermissions".to_string()),
        claude_dangerously_skip_permissions: Some(true),
        claude_allow_dangerously_skip_permissions: Some(true),
        ..input("claude")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert_eq!(merged.model, None);
    assert_eq!(merged.effort, Some(LogicalEffort::XHigh));
    assert_eq!(merged.approval_policy, None);
    assert_eq!(merged.sandbox_mode.as_deref(), Some("danger-full-access"));
    assert_eq!(
        merged.claude_permission_mode.as_deref(),
        Some("bypassPermissions")
    );
    assert!(merged.claude_dangerously_skip_permissions);
    assert!(merged.claude_allow_dangerously_skip_permissions);
}

#[test]
fn merge_clears_blank_sandbox_and_claude_permission_defaults() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.sandbox_mode = Some("workspace-write".to_string());
    settings.claude_permission_mode = Some("acceptEdits".to_string());
    let next = UpdateAgentProviderSettingsInput {
        sandbox_mode: Some(" ".to_string()),
        claude_permission_mode: Some(" ".to_string()),
        ..input("claude")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert_eq!(merged.sandbox_mode, None);
    assert_eq!(merged.claude_permission_mode, None);
}

#[test]
fn merge_clears_blank_effort_to_harness_default() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.effort = Some(LogicalEffort::High);
    let next = UpdateAgentProviderSettingsInput {
        effort: Some(" ".to_string()),
        ..input("codex")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert_eq!(merged.effort, None);
}

#[test]
fn merge_locks_codex_policy_and_sandbox_to_mcp_required_defaults() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    let next = UpdateAgentProviderSettingsInput {
        approval_policy: Some("on-request".to_string()),
        sandbox_mode: Some("workspace-write".to_string()),
        ..input("codex")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert_eq!(
        merged.approval_policy.as_deref(),
        Some(CODEX_DEFAULT_APPROVAL_POLICY)
    );
    assert_eq!(
        merged.sandbox_mode.as_deref(),
        Some(CODEX_DEFAULT_SANDBOX_MODE)
    );
}

#[test]
fn merge_reset_to_defaults_preserves_enabled_and_default_state() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.enabled = true;
    settings.is_default = true;
    settings.model = Some("claude-opus-4.5".to_string());
    settings.effort = Some(LogicalEffort::Max);
    settings.claude_permission_mode = Some("acceptEdits".to_string());
    settings.claude_dangerously_skip_permissions = false;
    let next = UpdateAgentProviderSettingsInput {
        reset_to_defaults: true,
        ..input("claude")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert!(merged.enabled);
    assert!(merged.is_default);
    assert_eq!(merged.model.as_deref(), Some("sonnet"));
    assert_eq!(merged.effort, Some(LogicalEffort::Medium));
    assert_eq!(
        merged.claude_permission_mode.as_deref(),
        Some("bypassPermissions")
    );
    assert!(merged.claude_dangerously_skip_permissions);
}

#[test]
fn merge_keeps_nonblank_string_defaults() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    let next = UpdateAgentProviderSettingsInput {
        model: Some("claude-sonnet-4.5".to_string()),
        approval_policy: Some("auto".to_string()),
        sandbox_mode: Some("workspace-write".to_string()),
        claude_permission_mode: Some("default".to_string()),
        ..input("claude")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert_eq!(merged.model.as_deref(), Some("claude-sonnet-4.5"));
    assert_eq!(merged.approval_policy.as_deref(), Some("auto"));
    assert_eq!(merged.sandbox_mode.as_deref(), Some("workspace-write"));
    assert_eq!(merged.claude_permission_mode.as_deref(), Some("default"));
}

#[test]
fn merge_disabling_provider_clears_default_flag() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.enabled = true;
    settings.is_default = true;
    let next = UpdateAgentProviderSettingsInput {
        enabled: Some(false),
        ..input("codex")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert!(!merged.enabled);
    assert!(!merged.is_default);
}

#[test]
fn parse_provider_and_effort_report_invalid_values() {
    assert_eq!(parse_provider("codex").unwrap(), AgentHarnessKind::Codex);
    assert!(parse_provider("bogus")
        .unwrap_err()
        .contains("Invalid provider"));
    assert_eq!(
        parse_effort(Some("high".to_string())).unwrap(),
        Some(LogicalEffort::High)
    );
    assert!(parse_effort(Some("bogus".to_string()))
        .unwrap_err()
        .contains("Invalid provider effort"));
}

#[test]
fn provider_status_uses_ready_path_or_error_message() {
    assert_eq!(
        provider_status(
            AgentHarnessKind::Codex,
            true,
            Some("/opt/homebrew/bin/codex"),
            None,
        ),
        "Available codex detected at /opt/homebrew/bin/codex."
    );
    assert_eq!(
        provider_status(AgentHarnessKind::Claude, true, None, None),
        "Available claude detected."
    );
    assert_eq!(
        provider_status(
            AgentHarnessKind::Codex,
            false,
            None,
            Some("codex missing core exec support"),
        ),
        "codex missing core exec support"
    );
    assert_eq!(
        provider_status(AgentHarnessKind::Claude, false, None, None),
        "claude CLI is not ready."
    );
}

#[test]
fn response_maps_settings_and_probe_fields() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.enabled = true;
    settings.is_default = true;
    settings.approval_policy = Some("never".to_string());
    settings.sandbox_mode = Some("danger-full-access".to_string());
    let response = to_response(
        settings,
        HarnessRuntimeProbe {
            binary_path: Some("/opt/homebrew/bin/codex".to_string()),
            binary_found: true,
            probe_succeeded: true,
            available: true,
            missing_core_exec_features: vec!["exec".to_string()],
            error: None,
        },
    );

    assert_eq!(response.provider, "codex");
    assert!(response.enabled);
    assert!(response.is_default);
    assert_eq!(response.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(response.effort.as_deref(), Some("xhigh"));
    assert_eq!(response.approval_policy.as_deref(), Some("never"));
    assert_eq!(response.sandbox_mode.as_deref(), Some("danger-full-access"));
    assert!(response.available);
    assert!(response.binary_found);
    assert_eq!(
        response.binary_path.as_deref(),
        Some("/opt/homebrew/bin/codex")
    );
    assert_eq!(
        response.status,
        "Available codex detected at /opt/homebrew/bin/codex."
    );
    assert_eq!(
        response.missing_core_exec_features,
        vec!["exec".to_string()]
    );
    assert!(!response.updated_at.is_empty());
}

#[test]
fn lane_settings_inherit_provider_defaults() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.model = Some("gpt-5.4".to_string());
    settings.effort = Some(LogicalEffort::High);
    settings.approval_policy = Some("never".to_string());
    settings.sandbox_mode = Some("danger-full-access".to_string());

    let lane = to_lane_settings(&settings, AgentLane::IdeationPrimary);

    assert_eq!(lane.harness, AgentHarnessKind::Codex);
    assert_eq!(lane.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(lane.effort, Some(LogicalEffort::High));
    assert_eq!(lane.approval_policy.as_deref(), Some("never"));
    assert_eq!(lane.sandbox_mode.as_deref(), Some("danger-full-access"));
}

#[tokio::test]
async fn read_settings_returns_ordered_provider_defaults() {
    let state = AppState::new_test();
    let response = read_provider_settings(&state)
        .await
        .expect("read provider settings");

    assert_eq!(response.providers.len(), 2);
    assert_eq!(response.providers[0].provider, "codex");
    assert_eq!(response.providers[1].provider, "claude");
    assert_eq!(response.default_provider.as_deref(), Some("claude"));
    assert!(!response.requires_onboarding);
}

#[tokio::test]
async fn read_settings_uses_fallback_probe_when_provider_probe_is_missing() {
    let state = AppState::new_test();
    let probes = HashMap::from([(AgentHarnessKind::Codex, ready_probe("/usr/bin/codex"))]);

    let response = read_provider_settings_with_probes(&state, &probes)
        .await
        .expect("read provider settings with custom probes");

    let claude = response
        .providers
        .iter()
        .find(|provider| provider.provider == "claude")
        .expect("claude response");
    assert!(!claude.available);
    assert_eq!(claude.error.as_deref(), Some("claude probe unavailable"));
}

#[tokio::test]
async fn update_settings_saves_default_and_applies_lanes_with_ready_probe() {
    let state = AppState::new_test();
    let probes = HashMap::from([
        (AgentHarnessKind::Codex, ready_probe("/usr/bin/codex")),
        (AgentHarnessKind::Claude, ready_probe("/usr/bin/claude")),
    ]);
    let next = UpdateAgentProviderSettingsInput {
        enabled: Some(true),
        is_default: Some(true),
        model: Some("gpt-5.4".to_string()),
        effort: Some("high".to_string()),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
        apply_to_all_lanes: true,
        ..input("codex")
    };

    let response = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect("update provider settings");

    assert_eq!(response.default_provider.as_deref(), Some("codex"));
    assert!(!response.requires_onboarding);
    for lane in AGENT_LANES {
        let stored = state
            .agent_lane_settings_repo
            .get_global(lane)
            .await
            .expect("read lane")
            .expect("lane settings");
        assert_eq!(stored.settings.harness, AgentHarnessKind::Codex);
        assert_eq!(stored.settings.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(stored.settings.effort, Some(LogicalEffort::High));
    }
}

#[tokio::test]
async fn update_first_enabled_provider_sets_default_and_applies_all_global_lanes() {
    let mut state = AppState::new_test();
    state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let probes = HashMap::from([
        (AgentHarnessKind::Codex, ready_probe("/usr/bin/codex")),
        (AgentHarnessKind::Claude, ready_probe("/usr/bin/claude")),
    ]);
    let next = UpdateAgentProviderSettingsInput {
        enabled: Some(true),
        model: Some("gpt-5.4".to_string()),
        effort: Some("high".to_string()),
        ..input("codex")
    };

    let response = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect("update provider settings");

    assert_eq!(response.default_provider.as_deref(), Some("codex"));
    let stored_default = state
        .agent_provider_settings_repo
        .get_default()
        .await
        .expect("read default")
        .expect("default provider");
    assert_eq!(stored_default.provider, AgentHarnessKind::Codex);
    for lane in AGENT_LANES {
        let stored = state
            .agent_lane_settings_repo
            .get_global(lane)
            .await
            .expect("read lane")
            .expect("lane settings");
        assert_eq!(stored.settings.harness, AgentHarnessKind::Codex);
        assert_eq!(stored.settings.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(stored.settings.effort, Some(LogicalEffort::High));
    }
}

#[tokio::test]
async fn update_settings_applies_claude_permission_defaults() {
    let state = AppState::new_test();
    let probes = HashMap::from([(AgentHarnessKind::Claude, ready_probe("/usr/bin/claude"))]);
    let next = UpdateAgentProviderSettingsInput {
        enabled: Some(true),
        is_default: Some(true),
        claude_permission_mode: Some("bypassPermissions".to_string()),
        claude_dangerously_skip_permissions: Some(true),
        claude_allow_dangerously_skip_permissions: Some(true),
        ..input("claude")
    };

    let response = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect("update claude provider settings");

    let claude = response
        .providers
        .iter()
        .find(|provider| provider.provider == "claude")
        .expect("claude provider");
    assert!(claude.is_default);
    assert_eq!(
        claude.claude_permission_mode.as_deref(),
        Some("bypassPermissions")
    );
    assert!(claude.claude_dangerously_skip_permissions);
    assert!(claude.claude_allow_dangerously_skip_permissions);
}

#[tokio::test]
async fn update_settings_rejects_missing_provider_probe() {
    let state = AppState::new_test();
    let probes = HashMap::new();

    let error = update_provider_settings_with_probes(input("codex"), &state, &probes)
        .await
        .expect_err("missing probe should be reported");

    assert_eq!(error, "codex probe unavailable");
}

#[tokio::test]
async fn apply_default_provider_to_all_global_lanes() {
    let state = AppState::new_test();
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.enabled = true;
    settings.is_default = true;
    settings.model = Some("gpt-5.4".to_string());
    settings.effort = Some(LogicalEffort::High);

    apply_provider_to_global_lanes(&state, &settings)
        .await
        .expect("apply provider to lanes");

    for lane in AGENT_LANES {
        let stored = state
            .agent_lane_settings_repo
            .get_global(lane)
            .await
            .expect("read lane")
            .expect("global lane settings");
        assert_eq!(stored.settings.harness, AgentHarnessKind::Codex);
        assert_eq!(stored.settings.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(stored.settings.effort, Some(LogicalEffort::High));
    }
}
