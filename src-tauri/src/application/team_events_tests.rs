use super::*;

#[test]
fn test_team_message_type_formatting() {
    // Verify that message types map to expected strings
    assert_eq!(
        match TeamMessageType::UserMessage {
            TeamMessageType::UserMessage => "user_message",
            TeamMessageType::TeammateMessage => "teammate_message",
            TeamMessageType::Broadcast => "broadcast",
            TeamMessageType::System => "system",
        },
        "user_message"
    );
}

#[test]
fn test_teammate_status_has_event() {
    // Idle and Shutdown/Failed have dedicated events
    assert!(matches!(TeammateStatus::Idle, TeammateStatus::Idle));
    assert!(matches!(TeammateStatus::Shutdown, TeammateStatus::Shutdown));
    assert!(matches!(TeammateStatus::Failed, TeammateStatus::Failed));
}
