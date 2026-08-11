use super::agent_capability_gate::{AgentCapabilities, AgentCapabilityGate};

#[test]
fn capability_gate_defaults_fail_closed() {
    let gate = AgentCapabilityGate::default();

    assert_eq!(gate.snapshot(), AgentCapabilities::default());
    assert!(!gate.team_enabled());
    assert!(!gate.workflows_enabled());
}

#[test]
fn capability_gate_updates_both_values_from_a_snapshot() {
    let gate = AgentCapabilityGate::default();

    gate.replace(AgentCapabilities {
        team: true,
        workflows: false,
        autopilot: false,
    });

    assert!(gate.team_enabled());
    assert!(!gate.workflows_enabled());

    gate.replace(AgentCapabilities {
        team: false,
        workflows: true,
        autopilot: true,
    });

    assert!(!gate.team_enabled());
    assert!(gate.workflows_enabled());
    assert!(gate.autopilot_enabled());
}
