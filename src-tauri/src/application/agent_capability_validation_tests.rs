use super::agent_capability_gate::{AgentCapabilities, AgentCapabilityGate};
use super::agent_capability_validation::{
    codex_ultra_support_for_model, validate_agent_capability, AgentCapabilityError,
};
use crate::domain::agents::AgentHarnessKind;
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
fn solo_remains_available_while_legacy_team_mode_is_read_only() {
    let gate = AgentCapabilityGate::default();

    assert!(validate_agent_capability(
        CoordinationMode::Solo,
        AgentHarnessKind::Claude,
        &gate,
        None,
    )
    .is_ok());
    assert_eq!(
        validate_agent_capability(
            CoordinationMode::LegacyClaudeTeam,
            AgentHarnessKind::Claude,
            &gate,
            None,
        ),
        Err(AgentCapabilityError::LegacyReadOnly)
    );
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
            AgentCapabilityError::LegacyReadOnly,
            "Legacy Claude team mode is read-only; switch this conversation to Defaults or Team.",
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
