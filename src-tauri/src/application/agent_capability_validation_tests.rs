use super::agent_capability_gate::{AgentCapabilities, AgentCapabilityGate};
use super::agent_capability_validation::{validate_agent_capability, AgentCapabilityError};
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
