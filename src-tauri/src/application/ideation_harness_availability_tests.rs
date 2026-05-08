use super::ideation_harness_availability::{
    build_harness_override_availability, build_lane_harness_availability,
    team_mode_supported_for_context, validate_chat_runtime_for_context,
    validate_claude_runtime_path, LaneHarnessAvailability, ResolvedLaneHarnessConfig,
};
use crate::application::harness_runtime_registry::{
    standard_harness_probe_registry, HarnessRuntimeProbe,
};
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, AgentLane};
use crate::domain::entities::ChatContextType;
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;
use std::collections::HashMap;
use std::sync::Arc;

fn unavailable_probe(error: &str) -> HarnessRuntimeProbe {
    HarnessRuntimeProbe {
        binary_path: None,
        binary_found: false,
        probe_succeeded: false,
        available: false,
        missing_core_exec_features: Vec::new(),
        error: Some(error.to_string()),
    }
}

fn probe_map(
    claude_probe: HarnessRuntimeProbe,
    codex_probe: HarnessRuntimeProbe,
) -> HashMap<AgentHarnessKind, HarnessRuntimeProbe> {
    crate::domain::agents::standard_harness_map(claude_probe, codex_probe)
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
        error: None,
    };
    let codex_probe = HarnessRuntimeProbe {
        binary_path: Some("/opt/homebrew/bin/codex".to_string()),
        binary_found: true,
        probe_succeeded: true,
        available: true,
        missing_core_exec_features: Vec::new(),
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
        error: None,
    };
    let codex_probe = HarnessRuntimeProbe {
        binary_path: Some("/opt/homebrew/bin/codex".to_string()),
        binary_found: true,
        probe_succeeded: true,
        available: false,
        missing_core_exec_features: vec!["json_output".to_string()],
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
async fn project_context_team_mode_uses_default_provider_harness() {
    let mut state = AppState::new_test();
    let provider_repo = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(AgentHarnessKind::Codex),
    );
    state.agent_provider_settings_repo = provider_repo as Arc<dyn AgentProviderSettingsRepository>;

    let supported = team_mode_supported_for_context(
        &state,
        ChatContextType::Project,
        "project-default-provider",
    )
    .await;

    assert!(!supported);
}
