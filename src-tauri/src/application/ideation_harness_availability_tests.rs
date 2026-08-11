use super::ideation_harness_availability::{
    build_harness_override_availability, build_lane_harness_availability,
    overlay_provider_runtime_probes, provider_aware_runtime_probes_for_repo,
    resolve_primary_ideation_harness_availability_for_state, validate_chat_runtime_for_context,
    validate_chat_runtime_for_context_with_override, validate_claude_runtime_path,
    LaneHarnessAvailability, ResolvedLaneHarnessConfig,
};
use crate::application::harness_runtime_registry::{
    standard_harness_probe_registry, HarnessRuntimeProbe,
};
use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentProviderCliManagementMode, AgentProviderSettings,
};
use crate::domain::entities::ChatContextType;
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;
use async_trait::async_trait;
use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::sync::Arc;

fn unavailable_probe(error: &str) -> HarnessRuntimeProbe {
    HarnessRuntimeProbe {
        binary_path: None,
        binary_found: false,
        probe_succeeded: false,
        available: false,
        missing_core_exec_features: Vec::new(),
        cli_version: None,
        supported_model_aliases: None,
        supported_efforts: None,
        ultra_supported_models: Vec::new(),
        supports_fast_mode: false,
        fast_mode_supported_models: Vec::new(),
        error: Some(error.to_string()),
    }
}

fn probe_map(
    claude_probe: HarnessRuntimeProbe,
    codex_probe: HarnessRuntimeProbe,
) -> HashMap<AgentHarnessKind, HarnessRuntimeProbe> {
    crate::domain::agents::standard_harness_map(claude_probe, codex_probe)
}

fn codex_provider_settings(
    mode: AgentProviderCliManagementMode,
    enabled: bool,
    default_provider: bool,
) -> AgentProviderSettings {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.enabled = enabled;
    settings.is_default = default_provider;
    settings.cli_management_mode = mode;
    settings
}

fn write_executable(path: &std::path::Path, contents: &str) {
    std::fs::write(path, contents).expect("write fake codex");
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

fn write_modern_codex_cli(path: &std::path::Path) {
    write_executable(
        path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-cli 0.144.0\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Codex CLI' 'Commands:' '  exec' '  resume' '  mcp' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --search' '      --add-dir <DIR>'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Run Codex non-interactively' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --add-dir <DIR>' '      --json'
elif [ "$1" = "features" ] && [ "$2" = "list" ]; then
  printf '%s\n' 'fast_mode stable true'
elif [ "$1" = "debug" ] && [ "$2" = "models" ] && [ -z "$3" ]; then
  printf '%s\n' '{"models":[{"slug":"gpt-5.5","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"}],"additional_speed_tiers":["fast"]}]}'
elif [ "$1" = "debug" ] && [ "$2" = "models" ] && [ "$3" = "--bundled" ]; then
  printf '%s\n' '{"models":[{"slug":"gpt-5.5","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"}],"additional_speed_tiers":["fast"]}]}'
else
  printf 'unexpected args: %s\n' "$*" >&2
  exit 64
fi
"#,
    );
}

struct FailingAgentProviderSettingsRepository;

fn provider_repo_error() -> Box<dyn Error> {
    Box::new(io::Error::other("provider repo failed"))
}

#[async_trait]
impl AgentProviderSettingsRepository for FailingAgentProviderSettingsRepository {
    async fn get(
        &self,
        _provider: AgentHarnessKind,
    ) -> Result<Option<AgentProviderSettings>, Box<dyn Error>> {
        Err(provider_repo_error())
    }

    async fn list(&self) -> Result<Vec<AgentProviderSettings>, Box<dyn Error>> {
        Err(provider_repo_error())
    }

    async fn get_default(&self) -> Result<Option<AgentProviderSettings>, Box<dyn Error>> {
        Err(provider_repo_error())
    }

    async fn upsert(
        &self,
        _settings: &AgentProviderSettings,
    ) -> Result<AgentProviderSettings, Box<dyn Error>> {
        Err(provider_repo_error())
    }
}

#[test]
fn codex_lane_uses_codex_when_core_exec_support_is_available() {
    let config = ResolvedLaneHarnessConfig {
        lane: AgentLane::IdeationPrimary,
        configured_harness: Some(AgentHarnessKind::Codex),
    };
    let claude_probe = HarnessRuntimeProbe {
        binary_path: Some("/opt/homebrew/bin/claude".to_string()),
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
    };
    let codex_probe = HarnessRuntimeProbe {
        binary_path: Some("/opt/homebrew/bin/codex".to_string()),
        binary_found: true,
        probe_succeeded: true,
        available: true,
        missing_core_exec_features: Vec::new(),
        cli_version: None,
        supported_model_aliases: None,
        supported_efforts: None,
        ultra_supported_models: Vec::new(),
        supports_fast_mode: true,
        fast_mode_supported_models: vec!["gpt-5.5".to_string()],
        error: None,
    };

    let availability =
        build_lane_harness_availability(config, &probe_map(claude_probe, codex_probe));

    assert_eq!(availability.effective_harness, AgentHarnessKind::Codex);
    assert!(availability.available);
    assert_eq!(
        availability.binary_path.as_deref(),
        Some("/opt/homebrew/bin/codex")
    );
}

#[test]
fn codex_lane_stays_unavailable_when_codex_is_unavailable() {
    let config = ResolvedLaneHarnessConfig {
        lane: AgentLane::IdeationVerifier,
        configured_harness: Some(AgentHarnessKind::Codex),
    };
    let claude_probe = HarnessRuntimeProbe {
        binary_path: Some("/opt/homebrew/bin/claude".to_string()),
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
    };
    let codex_probe = HarnessRuntimeProbe {
        binary_path: Some("/opt/homebrew/bin/codex".to_string()),
        binary_found: true,
        probe_succeeded: true,
        available: false,
        missing_core_exec_features: vec!["json_output".to_string()],
        cli_version: None,
        supported_model_aliases: None,
        supported_efforts: None,
        ultra_supported_models: Vec::new(),
        supports_fast_mode: false,
        fast_mode_supported_models: Vec::new(),
        error: Some("Codex CLI is missing required capability: json_output".to_string()),
    };

    let availability =
        build_lane_harness_availability(config, &probe_map(claude_probe, codex_probe));

    assert_eq!(availability.effective_harness, AgentHarnessKind::Codex);
    assert!(!availability.available);
    assert_eq!(
        availability.error.as_deref(),
        Some("Codex CLI is missing required capability: json_output")
    );
    assert_eq!(
        availability.missing_core_exec_features,
        vec!["json_output".to_string()]
    );
}

#[test]
fn default_lane_without_configuration_defaults_to_claude() {
    let config = ResolvedLaneHarnessConfig {
        lane: AgentLane::IdeationSubagent,
        configured_harness: None,
    };

    let availability = build_lane_harness_availability(
        config,
        &probe_map(
            unavailable_probe("Claude CLI not found"),
            unavailable_probe("Codex CLI not found"),
        ),
    );

    assert_eq!(availability.effective_harness, AgentHarnessKind::Claude);
    assert!(!availability.available);
    assert_eq!(availability.error.as_deref(), Some("Claude CLI not found"));
}

#[test]
fn validate_claude_runtime_path_accepts_available_claude() {
    let availability = LaneHarnessAvailability {
        lane: AgentLane::IdeationPrimary,
        configured_harness: Some(AgentHarnessKind::Claude),
        effective_harness: AgentHarnessKind::Claude,
        binary_path: None,
        binary_found: true,
        probe_succeeded: true,
        available: true,
        missing_core_exec_features: Vec::new(),
        error: None,
    };

    assert!(validate_claude_runtime_path(&availability, "unified ideation").is_ok());
}

#[test]
fn validate_claude_runtime_path_rejects_available_codex() {
    let availability = LaneHarnessAvailability {
        lane: AgentLane::IdeationPrimary,
        configured_harness: Some(AgentHarnessKind::Codex),
        effective_harness: AgentHarnessKind::Codex,
        binary_path: None,
        binary_found: true,
        probe_succeeded: true,
        available: true,
        missing_core_exec_features: Vec::new(),
        error: None,
    };

    let error = validate_claude_runtime_path(&availability, "unified ideation").unwrap_err();
    assert!(error.contains("unified ideation"));
    assert!(error.contains("Claude runtime"));
}

#[test]
fn standard_harness_probe_registry_keys_explicit_harnesses() {
    let registry = standard_harness_probe_registry();

    assert!(registry.contains_key(&AgentHarnessKind::Claude));
    assert!(registry.contains_key(&AgentHarnessKind::Codex));
}

#[test]
fn missing_requested_probe_does_not_silently_fall_back_to_default_probe() {
    let config = ResolvedLaneHarnessConfig {
        lane: AgentLane::IdeationPrimary,
        configured_harness: Some(AgentHarnessKind::Codex),
    };
    let probes = HashMap::from([(
        AgentHarnessKind::Claude,
        HarnessRuntimeProbe {
            binary_path: Some("/opt/homebrew/bin/claude".to_string()),
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
        },
    )]);

    let availability = build_lane_harness_availability(config, &probes);

    assert_eq!(availability.effective_harness, AgentHarnessKind::Codex);
    assert!(!availability.available);
    assert_eq!(
        availability.error.as_deref(),
        Some("No harness probe registered for codex")
    );
}

#[test]
fn project_chat_runtime_override_uses_requested_harness_probe() {
    let availability = build_harness_override_availability(
        ChatContextType::Project,
        AgentHarnessKind::Codex,
        &probe_map(
            unavailable_probe("Claude CLI not found"),
            HarnessRuntimeProbe {
                binary_path: Some("/opt/homebrew/bin/codex".to_string()),
                binary_found: true,
                probe_succeeded: true,
                available: true,
                missing_core_exec_features: Vec::new(),
                cli_version: None,
                supported_model_aliases: None,
                supported_efforts: None,
                ultra_supported_models: Vec::new(),
                supports_fast_mode: true,
                fast_mode_supported_models: vec!["gpt-5.5".to_string()],
                error: None,
            },
        ),
    );

    assert_eq!(availability.effective_harness, AgentHarnessKind::Codex);
    assert!(availability.available);
    assert_eq!(
        availability.binary_path.as_deref(),
        Some("/opt/homebrew/bin/codex")
    );
}

#[test]
fn provider_runtime_overlay_uses_rx_managed_codex_probe() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let managed_codex_path = temp_dir.path().join("codex");
    write_modern_codex_cli(&managed_codex_path);
    let _override =
        crate::application::managed_provider_cli::override_managed_codex_binary_path_for_tests(
            managed_codex_path.clone(),
        );
    let settings = codex_provider_settings(AgentProviderCliManagementMode::RxManaged, true, true);
    let mut probes = probe_map(
        unavailable_probe("Claude CLI not found"),
        unavailable_probe("Codex CLI not found"),
    );

    overlay_provider_runtime_probes(&[settings], &mut probes);

    let codex_probe = probes
        .get(&AgentHarnessKind::Codex)
        .expect("Codex probe should be overlaid");
    assert!(codex_probe.available);
    assert_eq!(
        codex_probe.binary_path.as_deref(),
        Some(managed_codex_path.to_string_lossy().as_ref())
    );
}

#[test]
fn provider_runtime_overlay_uses_custom_codex_probe() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let custom_codex_path = temp_dir.path().join("codex-wrapper");
    write_modern_codex_cli(&custom_codex_path);
    let mut settings =
        codex_provider_settings(AgentProviderCliManagementMode::UserManaged, true, true);
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(custom_codex_path.to_string_lossy().into_owned());
    let mut probes = probe_map(
        unavailable_probe("Claude CLI not found"),
        unavailable_probe("Codex CLI not found"),
    );

    overlay_provider_runtime_probes(&[settings], &mut probes);

    let codex_probe = probes
        .get(&AgentHarnessKind::Codex)
        .expect("Codex probe should be overlaid");
    assert!(codex_probe.available);
    assert_eq!(
        codex_probe.binary_path.as_deref(),
        Some(custom_codex_path.to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn provider_aware_runtime_probes_reports_provider_repo_errors() {
    let repo = Arc::new(FailingAgentProviderSettingsRepository)
        as Arc<dyn AgentProviderSettingsRepository>;

    let error = provider_aware_runtime_probes_for_repo(&repo)
        .await
        .expect_err("provider repo error should propagate");

    assert!(error.contains("Failed to read provider settings"));
    assert!(error.contains("provider repo failed"));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn primary_ideation_availability_uses_rx_managed_codex_probe() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let managed_codex_path = temp_dir.path().join("codex");
    write_modern_codex_cli(&managed_codex_path);
    let _override =
        crate::application::managed_provider_cli::override_managed_codex_binary_path_for_tests(
            managed_codex_path.clone(),
        );
    let state = AppState::new_test();
    let settings = codex_provider_settings(AgentProviderCliManagementMode::RxManaged, true, true);
    state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("upsert provider settings");
    state
        .agent_lane_settings_repo
        .upsert_global(
            AgentLane::IdeationPrimary,
            &crate::domain::agents::AgentLaneSettings::new(AgentHarnessKind::Codex),
        )
        .await
        .expect("upsert global lane settings");

    let availability = resolve_primary_ideation_harness_availability_for_state(&state, None)
        .await
        .expect("primary ideation availability should resolve");

    assert!(availability.available);
    assert_eq!(availability.effective_harness, AgentHarnessKind::Codex);
    assert_eq!(
        availability.binary_path.as_deref(),
        Some(managed_codex_path.to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn chat_runtime_validation_requires_enabled_default_provider_first() {
    let mut state = AppState::new_test();
    state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());

    let error = validate_chat_runtime_for_context(
        &state,
        ChatContextType::Project,
        "project-without-provider",
        "project chat",
    )
    .await
    .expect_err("missing default provider should block before runtime probe");

    assert!(error.contains("Settings > Harness > Providers"));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn chat_runtime_validation_reports_rx_managed_codex_missing_binary() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let missing_codex_path = temp_dir.path().join("missing-codex");
    let _override =
        crate::application::managed_provider_cli::override_managed_codex_binary_path_for_tests(
            missing_codex_path,
        );
    let state = AppState::new_test();
    let settings = codex_provider_settings(AgentProviderCliManagementMode::RxManaged, true, true);
    state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("upsert provider settings");

    let error = validate_chat_runtime_for_context_with_override(
        &state,
        ChatContextType::Project,
        "project-rx-managed-codex",
        "project chat",
        Some(AgentHarnessKind::Codex),
    )
    .await
    .expect_err("missing RX-managed Codex should block chat start");

    assert_eq!(error, "RX-managed Codex is not installed.");
}

#[tokio::test]
async fn chat_runtime_validation_still_rejects_disabled_codex_provider() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let custom_codex_path = temp_dir.path().join("codex-wrapper");
    write_modern_codex_cli(&custom_codex_path);
    let mut state = AppState::new_test();
    let provider_repo = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(AgentHarnessKind::Claude),
    );
    state.agent_provider_settings_repo =
        provider_repo.clone() as Arc<dyn AgentProviderSettingsRepository>;
    let mut disabled_codex =
        codex_provider_settings(AgentProviderCliManagementMode::UserManaged, false, false);
    disabled_codex.custom_binary_enabled = true;
    disabled_codex.custom_binary_path = Some(custom_codex_path.to_string_lossy().into_owned());
    provider_repo
        .upsert(&disabled_codex)
        .await
        .expect("upsert disabled Codex provider");

    let error = validate_chat_runtime_for_context_with_override(
        &state,
        ChatContextType::Project,
        "project-disabled-codex",
        "project chat",
        Some(AgentHarnessKind::Codex),
    )
    .await
    .expect_err("disabled provider should still block chat start");

    assert!(error.contains("codex is not enabled"));
}
