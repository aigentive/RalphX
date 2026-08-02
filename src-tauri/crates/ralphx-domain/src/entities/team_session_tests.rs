use super::TeamSessionStatus;

#[test]
fn team_session_rejects_invalid_and_accepts_lifecycle_transitions() {
    assert_eq!(
        TeamSessionStatus::Active.transition(TeamSessionStatus::Suspending),
        Ok(TeamSessionStatus::Suspending)
    );
    assert_eq!(
        TeamSessionStatus::Suspended.transition(TeamSessionStatus::Active),
        Ok(TeamSessionStatus::Active)
    );
    assert_eq!(
        TeamSessionStatus::Failed.transition(TeamSessionStatus::Draining),
        Ok(TeamSessionStatus::Draining)
    );
    assert!(TeamSessionStatus::Closed
        .transition(TeamSessionStatus::Active)
        .is_err());
    assert!(TeamSessionStatus::Active
        .transition(TeamSessionStatus::Closed)
        .is_err());
}
