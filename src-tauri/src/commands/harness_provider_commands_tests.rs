use crate::application::managed_provider_cli::override_managed_codex_binary_path_for_tests;
use crate::application::{harness_runtime_registry::HarnessRuntimeProbe, AppState, AGENT_LANES};
use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentProviderCliManagementMode, AgentProviderSettings,
    LogicalEffort, CODEX_DEFAULT_APPROVAL_POLICY, CODEX_DEFAULT_SANDBOX_MODE,
};
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::sync::Arc;

use super::{
    apply_provider_to_global_lanes, merge_input, parse_effort, parse_provider,
    provider_settings_snapshot_probe, provider_status, read_provider_settings,
    read_provider_settings_with_probes, snapshot_probes_from_provider_settings, to_lane_settings,
    to_response, update_provider_settings_with_probes, GetAgentProviderSettingsInput,
    UpdateAgentProviderSettingsInput,
};

#[test]
fn provider_settings_refresh_input_defaults_force_runtime_to_false() {
    let input: GetAgentProviderSettingsInput =
        serde_json::from_value(serde_json::json!({ "refreshRuntime": true }))
            .expect("deserialize refresh input");

    assert!(input.refresh_runtime);
    assert!(!input.force_runtime);
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
        service_tier: None,
        claude_permission_mode: None,
        claude_dangerously_skip_permissions: None,
        claude_allow_dangerously_skip_permissions: None,
        cli_management_mode: None,
        auto_update_enabled: None,
        custom_binary_enabled: None,
        custom_binary_path: None,
        custom_env_file_enabled: None,
        custom_env_file_path: None,
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
        cli_version: None,
        supported_model_aliases: None,
        supported_efforts: None,
        ultra_supported_models: Vec::new(),
        supports_fast_mode: false,
        fast_mode_supported_models: Vec::new(),
        error: None,
    }
}

fn fast_codex_probe(path: &str, supported_models: Vec<String>) -> HarnessRuntimeProbe {
    let mut probe = ready_probe(path);
    probe.supports_fast_mode = true;
    probe.fast_mode_supported_models = supported_models;
    probe
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
fn merge_sets_and_clears_service_tier() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    let next = UpdateAgentProviderSettingsInput {
        service_tier: Some(Some(" FAST ".to_string())),
        ..input("codex")
    };

    let merged = merge_input(settings, next, true).expect("merge service tier");
    assert_eq!(merged.service_tier.as_deref(), Some("fast"));

    let next = UpdateAgentProviderSettingsInput {
        service_tier: Some(None),
        ..input("codex")
    };
    let merged = merge_input(merged, next, true).expect("clear service tier");
    assert_eq!(merged.service_tier, None);
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
fn merge_accepts_rx_managed_cli_auto_update_policy() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    let next = UpdateAgentProviderSettingsInput {
        cli_management_mode: Some("rx_managed".to_string()),
        auto_update_enabled: Some(true),
        ..input("codex")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert_eq!(
        merged.cli_management_mode,
        AgentProviderCliManagementMode::RxManaged
    );
    assert!(merged.auto_update_enabled);
}

#[test]
fn merge_clears_auto_update_when_cli_is_user_managed() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.cli_management_mode = AgentProviderCliManagementMode::RxManaged;
    settings.auto_update_enabled = true;
    let next = UpdateAgentProviderSettingsInput {
        cli_management_mode: Some("user_managed".to_string()),
        auto_update_enabled: Some(true),
        ..input("claude")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert_eq!(
        merged.cli_management_mode,
        AgentProviderCliManagementMode::UserManaged
    );
    assert!(!merged.auto_update_enabled);
}

#[test]
fn merge_enabling_custom_binary_forces_user_managed_without_auto_update() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.cli_management_mode = AgentProviderCliManagementMode::RxManaged;
    settings.auto_update_enabled = true;
    let next = UpdateAgentProviderSettingsInput {
        custom_binary_enabled: Some(true),
        custom_binary_path: Some(Some(" /opt/tools/codex-wrapper ".to_string())),
        auto_update_enabled: Some(true),
        ..input("codex")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert!(merged.custom_binary_enabled);
    assert_eq!(
        merged.custom_binary_path.as_deref(),
        Some("/opt/tools/codex-wrapper")
    );
    assert_eq!(
        merged.cli_management_mode,
        AgentProviderCliManagementMode::UserManaged
    );
    assert!(!merged.auto_update_enabled);
}

#[test]
fn merge_rejects_custom_binary_enable_without_path() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    let next = UpdateAgentProviderSettingsInput {
        custom_binary_enabled: Some(true),
        custom_binary_path: Some(Some(" ".to_string())),
        ..input("claude")
    };

    let err = merge_input(settings, next, true).expect_err("path should be required");

    assert!(err.contains("Custom claude binary path is required"));
}

#[test]
fn merge_switching_to_rx_managed_disables_custom_binary_but_keeps_path() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some("/opt/tools/codex-wrapper".to_string());
    let next = UpdateAgentProviderSettingsInput {
        cli_management_mode: Some("rx_managed".to_string()),
        auto_update_enabled: Some(true),
        ..input("codex")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert!(!merged.custom_binary_enabled);
    assert_eq!(
        merged.custom_binary_path.as_deref(),
        Some("/opt/tools/codex-wrapper")
    );
    assert_eq!(
        merged.cli_management_mode,
        AgentProviderCliManagementMode::RxManaged
    );
    assert!(merged.auto_update_enabled);
}

#[test]
fn merge_accepts_custom_env_file_without_cli_mode_side_effects() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.cli_management_mode = AgentProviderCliManagementMode::RxManaged;
    settings.auto_update_enabled = true;
    let next = UpdateAgentProviderSettingsInput {
        custom_env_file_enabled: Some(true),
        custom_env_file_path: Some(Some(" /Users/example/.codex.env ".to_string())),
        ..input("codex")
    };

    let merged = merge_input(settings, next, true).expect("merge settings");

    assert!(merged.custom_env_file_enabled);
    assert_eq!(
        merged.custom_env_file_path.as_deref(),
        Some("/Users/example/.codex.env")
    );
    assert_eq!(
        merged.cli_management_mode,
        AgentProviderCliManagementMode::RxManaged
    );
    assert!(merged.auto_update_enabled);
}

#[test]
fn merge_rejects_custom_env_file_enable_without_path() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    let next = UpdateAgentProviderSettingsInput {
        custom_env_file_enabled: Some(true),
        custom_env_file_path: Some(Some(" ".to_string())),
        ..input("claude")
    };

    let err = merge_input(settings, next, true).expect_err("path should be required");

    assert!(err.contains("Custom claude env file path is required"));
}

#[test]
fn merge_rejects_invalid_cli_management_mode() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    let next = UpdateAgentProviderSettingsInput {
        cli_management_mode: Some("system".to_string()),
        ..input("codex")
    };

    let err = merge_input(settings, next, true).expect_err("mode should fail");

    assert!(err.contains("Invalid provider CLI management mode"));
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
    settings.cli_management_mode = AgentProviderCliManagementMode::RxManaged;
    settings.auto_update_enabled = true;
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some("/opt/tools/claude-wrapper".to_string());
    settings.custom_env_file_enabled = true;
    settings.custom_env_file_path = Some("/Users/example/.claude.env".to_string());
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
    assert_eq!(
        merged.cli_management_mode,
        AgentProviderCliManagementMode::UserManaged
    );
    assert!(!merged.auto_update_enabled);
    assert!(!merged.custom_binary_enabled);
    assert_eq!(merged.custom_binary_path, None);
    assert!(!merged.custom_env_file_enabled);
    assert_eq!(merged.custom_env_file_path, None);
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
            true,
            Some("/opt/homebrew/bin/codex"),
            None,
        ),
        "Available codex detected at /opt/homebrew/bin/codex."
    );
    assert_eq!(
        provider_status(AgentHarnessKind::Claude, true, true, None, None),
        "Available claude detected."
    );
    assert_eq!(
        provider_status(AgentHarnessKind::Claude, true, false, None, None),
        "claude is enabled in Settings."
    );
    assert_eq!(
        provider_status(
            AgentHarnessKind::Codex,
            false,
            false,
            None,
            Some("codex missing core exec support"),
        ),
        "codex missing core exec support"
    );
    assert_eq!(
        provider_status(AgentHarnessKind::Claude, false, false, None, None),
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
    settings.service_tier = Some("fast".to_string());
    settings.cli_management_mode = AgentProviderCliManagementMode::RxManaged;
    settings.auto_update_enabled = true;
    settings.custom_binary_path = Some("/opt/tools/codex-wrapper".to_string());
    settings.custom_env_file_enabled = true;
    settings.custom_env_file_path = Some("/Users/example/.codex.env".to_string());
    let response = to_response(
        settings,
        HarnessRuntimeProbe {
            binary_path: Some("/opt/homebrew/bin/codex".to_string()),
            binary_found: true,
            probe_succeeded: true,
            available: true,
            missing_core_exec_features: vec!["exec".to_string()],
            cli_version: Some("2.1.170".to_string()),
            supported_model_aliases: Some(vec!["sonnet".to_string(), "fable".to_string()]),
            supported_efforts: Some(vec!["low".to_string(), "medium".to_string()]),
            ultra_supported_models: vec!["gpt-5.6-sol".to_string()],
            supports_fast_mode: true,
            fast_mode_supported_models: vec!["gpt-5.4".to_string(), "gpt-5.5".to_string()],
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
    assert_eq!(response.service_tier.as_deref(), Some("fast"));
    assert_eq!(response.cli_management_mode, "rx_managed");
    assert!(response.auto_update_enabled);
    assert!(!response.custom_binary_enabled);
    assert_eq!(
        response.custom_binary_path.as_deref(),
        Some("/opt/tools/codex-wrapper")
    );
    assert!(response.custom_env_file_enabled);
    assert_eq!(
        response.custom_env_file_path.as_deref(),
        Some("/Users/example/.codex.env")
    );
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
    assert_eq!(response.cli_version.as_deref(), Some("2.1.170"));
    assert_eq!(
        response.supported_model_aliases,
        Some(vec!["sonnet".to_string(), "fable".to_string()])
    );
    assert_eq!(
        response.supported_efforts,
        Some(vec!["low".to_string(), "medium".to_string()])
    );
    assert_eq!(
        response.ultra_supported_models,
        vec!["gpt-5.6-sol".to_string()]
    );
    assert!(response.supports_fast_mode);
    assert_eq!(
        response.fast_mode_supported_models,
        vec!["gpt-5.4".to_string(), "gpt-5.5".to_string()]
    );
    assert!(!response.updated_at.is_empty());
}

#[test]
fn provider_settings_snapshot_probe_reports_disabled_provider() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);

    let probe = provider_settings_snapshot_probe(&settings);

    assert!(!probe.available);
    assert!(!probe.binary_found);
    assert!(!probe.probe_succeeded);
    assert_eq!(
        probe.error.as_deref(),
        Some("codex is disabled. Enable and validate it in Settings before use.")
    );
}

#[test]
fn snapshot_probes_fill_missing_standard_providers_as_disabled() {
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;

    let probes = snapshot_probes_from_provider_settings(&[codex]);

    let codex_probe = probes
        .get(&AgentHarnessKind::Codex)
        .expect("codex probe should be present");
    assert!(codex_probe.available);

    let claude_probe = probes
        .get(&AgentHarnessKind::Claude)
        .expect("claude fallback probe should be present");
    assert!(!claude_probe.available);
    assert_eq!(
        claude_probe.error.as_deref(),
        Some("claude is disabled. Enable and validate it in Settings before use.")
    );
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
    let response = read_provider_settings(&state, false, false)
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
#[allow(clippy::await_holding_lock)]
async fn update_settings_saves_default_and_applies_lanes_with_ready_probe() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let managed_codex_path = temp_dir.path().join("codex");
    write_modern_codex_cli(&managed_codex_path);
    let _override = override_managed_codex_binary_path_for_tests(managed_codex_path);
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
        cli_management_mode: Some("rx_managed".to_string()),
        auto_update_enabled: Some(true),
        apply_to_all_lanes: true,
        ..input("codex")
    };

    let response = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect("update provider settings");

    assert_eq!(response.default_provider.as_deref(), Some("codex"));
    assert!(!response.requires_onboarding);
    let codex = response
        .providers
        .iter()
        .find(|provider| provider.provider == "codex")
        .expect("codex provider");
    assert_eq!(codex.cli_management_mode, "rx_managed");
    assert!(codex.auto_update_enabled);
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
async fn update_settings_rejects_codex_fast_without_fast_capability() {
    let state = AppState::new_test();
    let probes = HashMap::from([
        (AgentHarnessKind::Codex, ready_probe("/usr/bin/codex")),
        (AgentHarnessKind::Claude, ready_probe("/usr/bin/claude")),
    ]);
    let next = UpdateAgentProviderSettingsInput {
        enabled: Some(true),
        service_tier: Some(Some("fast".to_string())),
        ..input("codex")
    };

    let err = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect_err("unsupported Fast mode should be rejected");

    assert!(err.contains("Codex Fast mode is not supported"));
}

#[tokio::test]
async fn update_settings_rejects_codex_fast_for_unsupported_model() {
    let state = AppState::new_test();
    let probes = HashMap::from([
        (
            AgentHarnessKind::Codex,
            fast_codex_probe("/usr/bin/codex", vec!["gpt-5.5".to_string()]),
        ),
        (AgentHarnessKind::Claude, ready_probe("/usr/bin/claude")),
    ]);
    let next = UpdateAgentProviderSettingsInput {
        enabled: Some(true),
        model: Some("gpt-5.4-mini".to_string()),
        service_tier: Some(Some("fast".to_string())),
        ..input("codex")
    };

    let err = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect_err("model without Fast tier should be rejected");

    assert_eq!(
        err,
        "Codex Fast mode is not available for model gpt-5.4-mini."
    );
}

#[tokio::test]
async fn update_settings_saves_custom_binary_after_candidate_probe() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let codex_path = temp_dir.path().join("codex-wrapper");
    write_modern_codex_cli(&codex_path);
    let state = AppState::new_test();
    let probes = HashMap::from([
        (
            AgentHarnessKind::Codex,
            HarnessRuntimeProbe {
                available: false,
                binary_found: false,
                probe_succeeded: false,
                binary_path: None,
                missing_core_exec_features: Vec::new(),
                cli_version: None,
                supported_model_aliases: None,
                supported_efforts: None,
                ultra_supported_models: Vec::new(),
                supports_fast_mode: false,
                fast_mode_supported_models: Vec::new(),
                error: Some("PATH Codex unavailable".to_string()),
            },
        ),
        (AgentHarnessKind::Claude, ready_probe("/usr/bin/claude")),
    ]);
    let next = UpdateAgentProviderSettingsInput {
        enabled: Some(true),
        custom_binary_enabled: Some(true),
        custom_binary_path: Some(Some(codex_path.to_string_lossy().into_owned())),
        cli_management_mode: Some("rx_managed".to_string()),
        auto_update_enabled: Some(true),
        ..input("codex")
    };

    let response = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect("update provider settings");

    let codex = response
        .providers
        .iter()
        .find(|provider| provider.provider == "codex")
        .expect("codex provider");
    assert!(codex.enabled);
    assert!(codex.custom_binary_enabled);
    assert_eq!(
        codex.custom_binary_path.as_deref(),
        Some(codex_path.to_string_lossy().as_ref())
    );
    assert_eq!(codex.cli_management_mode, "user_managed");
    assert!(!codex.auto_update_enabled);
    assert!(codex.available);
    assert_eq!(
        codex.binary_path.as_deref(),
        Some(codex_path.to_string_lossy().as_ref())
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn update_settings_expands_home_relative_custom_binary_before_save() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let home_dir = temp_dir.path().join("home");
    let bin_dir = home_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    let _home = EnvGuard::set_os("HOME", &home_dir);
    let codex_path = bin_dir.join("codex-wrapper");
    write_modern_codex_cli(&codex_path);
    let state = AppState::new_test();
    let probes = HashMap::from([
        (
            AgentHarnessKind::Codex,
            HarnessRuntimeProbe {
                available: false,
                binary_found: false,
                probe_succeeded: false,
                binary_path: None,
                missing_core_exec_features: Vec::new(),
                cli_version: None,
                supported_model_aliases: None,
                supported_efforts: None,
                ultra_supported_models: Vec::new(),
                supports_fast_mode: false,
                fast_mode_supported_models: Vec::new(),
                error: Some("PATH Codex unavailable".to_string()),
            },
        ),
        (AgentHarnessKind::Claude, ready_probe("/usr/bin/claude")),
    ]);
    let next = UpdateAgentProviderSettingsInput {
        enabled: Some(true),
        custom_binary_enabled: Some(true),
        custom_binary_path: Some(Some("~/bin/codex-wrapper".to_string())),
        ..input("codex")
    };

    let response = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect("update provider settings");

    let codex = response
        .providers
        .iter()
        .find(|provider| provider.provider == "codex")
        .expect("codex provider");
    assert!(codex.custom_binary_enabled);
    assert_eq!(
        codex.custom_binary_path.as_deref(),
        Some(codex_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        codex.binary_path.as_deref(),
        Some(codex_path.to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn update_settings_rejects_invalid_custom_binary_candidate_before_save() {
    let state = AppState::new_test();
    let probes = HashMap::from([(AgentHarnessKind::Codex, ready_probe("/usr/bin/codex"))]);
    let next = UpdateAgentProviderSettingsInput {
        custom_binary_enabled: Some(true),
        custom_binary_path: Some(Some("relative/codex".to_string())),
        ..input("codex")
    };

    let error = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect_err("relative custom path should fail");
    let stored = state
        .agent_provider_settings_repo
        .get(AgentHarnessKind::Codex)
        .await
        .expect("read provider settings")
        .expect("seeded provider settings");

    assert!(error.contains("absolute path"));
    assert!(!stored.custom_binary_enabled);
    assert_eq!(stored.custom_binary_path, None);
}

#[tokio::test]
async fn update_settings_rejects_unsupported_tilde_custom_binary_before_save() {
    let state = AppState::new_test();
    let probes = HashMap::from([(AgentHarnessKind::Codex, ready_probe("/usr/bin/codex"))]);
    let next = UpdateAgentProviderSettingsInput {
        custom_binary_enabled: Some(true),
        custom_binary_path: Some(Some("~other/bin/codex".to_string())),
        ..input("codex")
    };

    let error = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect_err("tilde-user custom path should fail");
    let stored = state
        .agent_provider_settings_repo
        .get(AgentHarnessKind::Codex)
        .await
        .expect("read provider settings")
        .expect("seeded provider settings");

    assert!(error.contains("only ~/"));
    assert!(!stored.custom_binary_enabled);
    assert_eq!(stored.custom_binary_path, None);
}

#[tokio::test]
async fn update_settings_validates_custom_env_file_candidate_before_save() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let env_path = temp_dir.path().join("codex.env");
    std::fs::write(&env_path, "ANTHROPIC_AUTH_TOKEN=secret\n").expect("write env file");
    let state = AppState::new_test();
    let probes = HashMap::from([(AgentHarnessKind::Codex, ready_probe("/usr/bin/codex"))]);
    let next = UpdateAgentProviderSettingsInput {
        custom_env_file_enabled: Some(true),
        custom_env_file_path: Some(Some(env_path.to_string_lossy().into_owned())),
        ..input("codex")
    };

    let response = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect("update provider settings");

    let codex = response
        .providers
        .iter()
        .find(|provider| provider.provider == "codex")
        .expect("codex provider");
    assert!(codex.custom_env_file_enabled);
    assert_eq!(
        codex.custom_env_file_path.as_deref(),
        Some(env_path.to_string_lossy().as_ref())
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn update_settings_expands_home_relative_custom_env_file_before_save() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let home_dir = temp_dir.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("create home dir");
    let _home = EnvGuard::set_os("HOME", &home_dir);
    let env_path = home_dir.join(".codex.env");
    std::fs::write(&env_path, "ANTHROPIC_AUTH_TOKEN=secret\n").expect("write env file");
    let state = AppState::new_test();
    let probes = HashMap::from([(AgentHarnessKind::Codex, ready_probe("/usr/bin/codex"))]);
    let next = UpdateAgentProviderSettingsInput {
        custom_env_file_enabled: Some(true),
        custom_env_file_path: Some(Some("~/.codex.env".to_string())),
        ..input("codex")
    };

    let response = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect("update provider settings");

    let codex = response
        .providers
        .iter()
        .find(|provider| provider.provider == "codex")
        .expect("codex provider");
    assert!(codex.custom_env_file_enabled);
    assert_eq!(
        codex.custom_env_file_path.as_deref(),
        Some(env_path.to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn update_settings_rejects_invalid_custom_env_file_candidate_before_save() {
    let state = AppState::new_test();
    let probes = HashMap::from([(AgentHarnessKind::Codex, ready_probe("/usr/bin/codex"))]);
    let next = UpdateAgentProviderSettingsInput {
        custom_env_file_enabled: Some(true),
        custom_env_file_path: Some(Some("relative.env".to_string())),
        ..input("codex")
    };

    let error = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect_err("relative env path should fail");
    let stored = state
        .agent_provider_settings_repo
        .get(AgentHarnessKind::Codex)
        .await
        .expect("read provider settings")
        .expect("seeded provider settings");

    assert!(error.contains("absolute"));
    assert!(!stored.custom_env_file_enabled);
    assert_eq!(stored.custom_env_file_path, None);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn update_settings_rejects_home_relative_env_path_with_parent_component_before_save() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let home_dir = temp_dir.path().join("home");
    std::fs::create_dir_all(&home_dir).expect("create home dir");
    let _home = EnvGuard::set_os("HOME", &home_dir);
    let state = AppState::new_test();
    let probes = HashMap::from([(AgentHarnessKind::Codex, ready_probe("/usr/bin/codex"))]);
    let next = UpdateAgentProviderSettingsInput {
        custom_env_file_enabled: Some(true),
        custom_env_file_path: Some(Some("~/../codex.env".to_string())),
        ..input("codex")
    };

    let error = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect_err("home-relative traversal should fail");
    let stored = state
        .agent_provider_settings_repo
        .get(AgentHarnessKind::Codex)
        .await
        .expect("read provider settings")
        .expect("seeded provider settings");

    assert!(error.contains("unsafe components"));
    assert!(!stored.custom_env_file_enabled);
    assert_eq!(stored.custom_env_file_path, None);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn update_settings_reprobes_managed_cli_after_mode_switch() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let missing_codex_path = temp_dir.path().join("missing-codex");
    let _override = override_managed_codex_binary_path_for_tests(missing_codex_path.clone());
    let state = AppState::new_test();
    let mut stored = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    stored.enabled = true;
    stored.is_default = true;
    stored.cli_management_mode = AgentProviderCliManagementMode::UserManaged;
    state
        .agent_provider_settings_repo
        .upsert(&stored)
        .await
        .expect("save existing provider");
    let probes = HashMap::from([
        (AgentHarnessKind::Codex, ready_probe("/usr/bin/codex")),
        (AgentHarnessKind::Claude, ready_probe("/usr/bin/claude")),
    ]);
    let next = UpdateAgentProviderSettingsInput {
        cli_management_mode: Some("rx_managed".to_string()),
        auto_update_enabled: Some(false),
        ..input("codex")
    };

    let response = update_provider_settings_with_probes(next, &state, &probes)
        .await
        .expect("update provider settings");

    let codex = response
        .providers
        .iter()
        .find(|provider| provider.provider == "codex")
        .expect("codex provider");
    assert_eq!(codex.cli_management_mode, "rx_managed");
    assert!(codex.enabled);
    assert!(!codex.available);
    assert_eq!(
        codex.binary_path.as_deref(),
        Some(missing_codex_path.to_string_lossy().as_ref())
    );
    assert_eq!(codex.status, "RX-managed Codex is not installed.");
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

fn write_modern_codex_cli(path: &std::path::Path) {
    std::fs::write(
        path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-cli 0.144.0\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Codex CLI' 'Commands:' '  exec' '  resume' '  mcp' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --search' '      --add-dir <DIR>'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Run Codex non-interactively' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --add-dir <DIR>' '      --json'
else
  printf 'unexpected args: %s\n' "$*" >&2
  exit 64
fi
"#,
    )
    .expect("write fake codex");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .expect("fake codex metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod fake codex");
    }
}

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvGuard {
    fn set_os(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}
