use super::{normalize_team_member_name, AgentRunId, TeamMemberStatus};

#[test]
fn team_member_name_normalization_is_bounded_and_unambiguous() {
    assert_eq!(
        normalize_team_member_name("  API   Worker ").unwrap(),
        "api worker"
    );
    assert!(normalize_team_member_name(" ").is_err());
    assert!(normalize_team_member_name("member/name").is_err());
    assert!(normalize_team_member_name(&"a".repeat(97)).is_err());
}

#[test]
fn team_member_status_transitions_reject_terminal_revival() {
    assert!(TeamMemberStatus::Idle.can_transition_to(TeamMemberStatus::Working));
    assert!(TeamMemberStatus::Working.can_transition_to(TeamMemberStatus::Idle));
    assert!(!TeamMemberStatus::Stopped.can_transition_to(TeamMemberStatus::Idle));
    assert_ne!(AgentRunId::new(), AgentRunId::new());
}
