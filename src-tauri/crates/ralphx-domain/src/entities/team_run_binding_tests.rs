use super::{TeamRunBindingStatus, TeamWorkClassification};

#[test]
fn team_run_binding_enforces_launch_lifecycle_and_classification() {
    assert!(TeamRunBindingStatus::Planned.can_transition_to(TeamRunBindingStatus::Launching));
    assert!(TeamRunBindingStatus::Launching.can_transition_to(TeamRunBindingStatus::Running));
    assert!(!TeamRunBindingStatus::Terminal.can_transition_to(TeamRunBindingStatus::Running));
    assert_ne!(
        TeamWorkClassification::CoordinationOnly,
        TeamWorkClassification::Write
    );
}
