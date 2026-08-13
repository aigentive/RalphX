use super::agent_capability_gate::{AgentCapabilities, AgentCapabilityGate};
use super::agent_capability_validation::{
    codex_fast_support_for_probe, codex_ultra_support_for_model, validate_agent_capability,
    validate_manual_role_runtime_capabilities, AgentCapabilityError,
};
use super::harness_runtime_registry::HarnessRuntimeProbe;
use crate::domain::agents::{AgentHarnessKind, ManualRoleDefault, ManualServiceTier};
use crate::domain::entities::CoordinationMode;

#[test]
fn team_and_workflow_capabilities_fail_closed_and_enable_independently() {
    let gate = AgentCapabilityGate::default();

    assert_eq!(
        validate_agent_capability(
            CoordinationMode::RxNativeTeam,
            AgentHarnessKind::Claude,
            &gate,
            None,
        ),
        Err(AgentCapabilityError::TeamDisabled)
    );
    assert_eq!(
        validate_agent_capability(
            CoordinationMode::RxNativeWorkflow,
            AgentHarnessKind::Codex,
            &gate,
            None,
        ),
        Err(AgentCapabilityError::WorkflowsDisabled)
    );

    gate.replace(AgentCapabilities {
        team: false,
        workflows: true,
        autopilot: false,
    });
    assert!(validate_agent_capability(
        CoordinationMode::RxNativeWorkflow,
        AgentHarnessKind::Claude,
        &gate,
        None,
    )
    .is_ok());
    assert_eq!(
        validate_agent_capability(
            CoordinationMode::RxNativeTeam,
            AgentHarnessKind::Codex,
            &gate,
            None,
        ),
        Err(AgentCapabilityError::TeamDisabled)
    );
}

#[test]
fn ultra_requires_codex_and_positive_live_model_support() {
    let gate = AgentCapabilityGate::default();

    assert_eq!(
        validate_agent_capability(
            CoordinationMode::CodexNativeUltra,
            AgentHarnessKind::Claude,
            &gate,
            Some(true),
        ),
        Err(AgentCapabilityError::UltraRequiresCodex)
    );
    assert_eq!(
        validate_agent_capability(
            CoordinationMode::CodexNativeUltra,
            AgentHarnessKind::Codex,
            &gate,
            Some(false),
        ),
        Err(AgentCapabilityError::UltraUnavailable)
    );
    assert_eq!(
        validate_agent_capability(
            CoordinationMode::CodexNativeUltra,
            AgentHarnessKind::Codex,
            &gate,
            None,
        ),
        Err(AgentCapabilityError::UltraUnavailable)
    );
    assert!(validate_agent_capability(
        CoordinationMode::CodexNativeUltra,
        AgentHarnessKind::Codex,
        &gate,
        Some(true),
    )
    .is_ok());
}

#[test]
fn capability_errors_explain_the_required_user_action() {
    let cases = [
        (
            AgentCapabilityError::TeamDisabled,
            "Team is disabled. Enable it in Settings > Capabilities or switch this conversation to Defaults.",
        ),
        (
            AgentCapabilityError::WorkflowsDisabled,
            "Workflows are disabled. Enable them in Settings > Capabilities or switch this conversation to Defaults.",
        ),
        (
            AgentCapabilityError::UltraRequiresCodex,
            "Codex Ultra is available only with the Codex provider.",
        ),
        (
            AgentCapabilityError::UltraUnavailable,
            "Codex Ultra is unavailable for the selected model and Codex account.",
        ),
    ];

    for (error, message) in cases {
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn ultra_model_support_requires_a_codex_model_selection() {
    assert_eq!(
        codex_ultra_support_for_model(AgentHarnessKind::Claude, Some("gpt-5.4")),
        None
    );
    assert_eq!(
        codex_ultra_support_for_model(AgentHarnessKind::Codex, None),
        None
    );
    assert_eq!(
        codex_ultra_support_for_model(AgentHarnessKind::Codex, Some("  ")),
        None
    );
}

#[test]
fn manual_role_workflow_validation_tracks_the_live_gate() {
    let gate = AgentCapabilityGate::default();
    let value = ManualRoleDefault {
        harness: AgentHarnessKind::Claude,
        model: Some("sonnet".to_string()),
        effort: None,
        service_tier: ManualServiceTier::Standard,
        coordination_mode: Some(CoordinationMode::RxNativeWorkflow),
        persona_id: None,
        approval_policy: None,
        sandbox_mode: None,
        atlassian_access: None,
    };

    assert!(validate_manual_role_runtime_capabilities(&value, &gate)
        .unwrap_err()
        .contains("Workflows are disabled"));
    gate.replace(AgentCapabilities {
        team: false,
        workflows: true,
        autopilot: false,
    });
    assert!(validate_manual_role_runtime_capabilities(&value, &gate).is_ok());
}

#[test]
fn fast_model_support_uses_the_probed_cli_catalog() {
    let probe = HarnessRuntimeProbe {
        binary_path: Some("/tmp/codex".to_string()),
        binary_found: true,
        probe_succeeded: true,
        available: true,
        missing_core_exec_features: Vec::new(),
        cli_version: Some("1.0.0".to_string()),
        supported_model_aliases: None,
        supported_efforts: None,
        ultra_supported_models: Vec::new(),
        supports_fast_mode: true,
        fast_mode_supported_models: vec!["gpt-5.5".to_string()],
        error: None,
    };

    assert!(codex_fast_support_for_probe(Some("gpt-5.5"), &probe));
    assert!(!codex_fast_support_for_probe(Some("gpt-5.4"), &probe));
    assert!(codex_fast_support_for_probe(None, &probe));

    let unavailable = HarnessRuntimeProbe {
        supports_fast_mode: false,
        ..probe
    };
    assert!(!codex_fast_support_for_probe(Some("gpt-5.5"), &unavailable));
}
