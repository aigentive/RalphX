use super::TeamMessageDeliveryStatus;

#[test]
fn team_delivery_state_machine_rejects_acknowledgement_before_delivery() {
    assert!(TeamMessageDeliveryStatus::Pending.can_transition_to(TeamMessageDeliveryStatus::Queued));
    assert!(
        TeamMessageDeliveryStatus::Queued.can_transition_to(TeamMessageDeliveryStatus::Delivered)
    );
    assert!(TeamMessageDeliveryStatus::Delivered
        .can_transition_to(TeamMessageDeliveryStatus::Acknowledged));
    assert!(!TeamMessageDeliveryStatus::Pending
        .can_transition_to(TeamMessageDeliveryStatus::Acknowledged));
    assert!(TeamMessageDeliveryStatus::Failed.can_transition_to(TeamMessageDeliveryStatus::Pending));
}
