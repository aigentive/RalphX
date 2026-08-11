use super::TeamWakeBatchStatus;

#[test]
fn team_wake_batch_state_machine_keeps_one_active_lifecycle() {
    assert!(TeamWakeBatchStatus::Queued.is_active());
    assert!(TeamWakeBatchStatus::Launching.is_active());
    assert!(!TeamWakeBatchStatus::Settled.is_active());
    assert!(TeamWakeBatchStatus::Running.can_transition_to(TeamWakeBatchStatus::Settled));
    assert!(!TeamWakeBatchStatus::Settled.can_transition_to(TeamWakeBatchStatus::Queued));
}
