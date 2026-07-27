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
