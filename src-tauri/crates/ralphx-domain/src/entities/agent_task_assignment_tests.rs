use super::{AgentTaskAssignmentId, AgentTaskAssignmentState, AgentTaskAssignmentTerminalStatus};

#[test]
fn assignment_states_parse_and_classify_unresolved_attempts() {
    for state in [
        AgentTaskAssignmentState::Reserved,
        AgentTaskAssignmentState::Active,
        AgentTaskAssignmentState::CompletionRequested,
        AgentTaskAssignmentState::ReleaseRequested,
    ] {
        assert!(state.is_unresolved(), "{state} should remain unresolved");
        assert_eq!(
            state.as_str().parse::<AgentTaskAssignmentState>(),
            Ok(state)
        );
    }

    for state in [
        AgentTaskAssignmentState::Completed,
        AgentTaskAssignmentState::Released,
        AgentTaskAssignmentState::Failed,
        AgentTaskAssignmentState::Cancelled,
    ] {
        assert!(!state.is_unresolved(), "{state} should be terminal");
        assert_eq!(
            state.as_str().parse::<AgentTaskAssignmentState>(),
            Ok(state)
        );
    }

    assert!("unknown".parse::<AgentTaskAssignmentState>().is_err());
}

#[test]
fn assignment_ids_and_terminal_status_are_typed() {
    let id = AgentTaskAssignmentId::from_string("assignment-1");
    assert_eq!(id.as_str(), "assignment-1");
    assert_eq!(id.to_string(), "assignment-1");
    assert!(!AgentTaskAssignmentId::new().as_str().is_empty());
    assert!(!AgentTaskAssignmentId::default().as_str().is_empty());
    assert_eq!(AgentTaskAssignmentState::Reserved.to_string(), "reserved");
    assert_eq!(
        AgentTaskAssignmentTerminalStatus::Completed,
        AgentTaskAssignmentTerminalStatus::Completed
    );
}

#[test]
fn assignment_team_linkage_stays_optional_for_legacy_assignments() {
    let team_id = super::TeamSessionId::from_string("team-1");
    let member_id = super::TeamMemberId::from_string("member-1");
    assert_eq!(team_id.as_str(), "team-1");
    assert_eq!(member_id.as_str(), "member-1");
}
