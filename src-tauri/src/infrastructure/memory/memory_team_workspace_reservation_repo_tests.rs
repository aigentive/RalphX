use crate::domain::entities::{AgentTaskAssignmentId, TeamWorkspaceReservationId};
use crate::domain::repositories::TeamWorkspaceReservationRepository;
use crate::infrastructure::memory::MemoryTeamWorkspaceReservationRepository;
use crate::testing::team_fixtures::team_reservation;

#[tokio::test]
async fn test_overlapping_paths_and_locks_conflict() {
    let repo = MemoryTeamWorkspaceReservationRepository::new();

    let mut first = team_reservation("reservation-1", "team-1", "member-1");
    first.resource_locks = vec!["cargo-target".to_string()];
    repo.acquire(first).await.unwrap();

    let mut nested = team_reservation("reservation-2", "team-1", "member-2");
    nested.writable_paths = vec!["src/module_a/inner.rs".to_string()];
    assert!(repo.acquire(nested).await.is_err());

    let mut lock_overlap = team_reservation("reservation-3", "team-1", "member-2");
    lock_overlap.writable_paths = vec!["src/other".to_string()];
    lock_overlap.resource_locks = vec!["cargo-target".to_string()];
    assert!(repo.acquire(lock_overlap).await.is_err());

    let mut sibling = team_reservation("reservation-4", "team-1", "member-2");
    sibling.writable_paths = vec!["src/module_ab".to_string()];
    repo.acquire(sibling).await.unwrap();

    let mut other_team = team_reservation("reservation-5", "team-2", "member-9");
    other_team.writable_paths = vec!["src/module_a".to_string()];
    repo.acquire(other_team).await.unwrap();
}

#[tokio::test]
async fn test_release_guards_and_assignment_listing() {
    let repo = MemoryTeamWorkspaceReservationRepository::new();
    let id = TeamWorkspaceReservationId::from_string("reservation-1");

    let mut reservation = team_reservation("reservation-1", "team-1", "member-1");
    reservation.assignment_id = Some(AgentTaskAssignmentId::from_string("assignment-1"));
    repo.acquire(reservation).await.unwrap();

    assert_eq!(
        repo.list_active_for_assignment("assignment-1")
            .await
            .unwrap()
            .len(),
        1
    );

    assert!(!repo.release_if_current(&id, 1, 1).await.unwrap());
    assert!(!repo.release_if_current(&id, 0, 2).await.unwrap());
    assert!(repo.release_if_current(&id, 0, 1).await.unwrap());
    assert!(!repo.release_if_current(&id, 0, 1).await.unwrap());

    assert!(repo
        .list_active_for_assignment("assignment-1")
        .await
        .unwrap()
        .is_empty());

    // Path is free after release.
    repo.acquire(team_reservation("reservation-2", "team-1", "member-2"))
        .await
        .unwrap();
}
