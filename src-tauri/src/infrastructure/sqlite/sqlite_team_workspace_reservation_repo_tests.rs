use crate::domain::entities::{AgentTaskAssignmentId, TeamWorkspaceReservationId};
use crate::domain::repositories::TeamWorkspaceReservationRepository;
use crate::infrastructure::sqlite::SqliteTeamWorkspaceReservationRepository;
use crate::testing::team_fixtures::{
    seed_team_member_row, seed_team_session_row, team_reservation,
};
use crate::testing::SqliteTestDb;

fn setup_repo() -> (SqliteTestDb, SqliteTeamWorkspaceReservationRepository) {
    let db = SqliteTestDb::new("sqlite-team-workspace-reservation-repo");
    db.with_connection(|conn| {
        seed_team_session_row(conn, "team-1", 101);
        seed_team_session_row(conn, "team-2", 102);
        seed_team_member_row(conn, "member-1", "team-1");
        seed_team_member_row(conn, "member-2", "team-1");
        seed_team_member_row(conn, "member-9", "team-2");
    });
    let repo = SqliteTeamWorkspaceReservationRepository::from_shared(db.shared_conn());
    (db, repo)
}

#[tokio::test]
async fn test_acquire_and_roundtrip() {
    let (_db, repo) = setup_repo();

    let created = repo
        .acquire(team_reservation("reservation-1", "team-1", "member-1"))
        .await
        .unwrap();
    let fetched = repo
        .get_by_id(&TeamWorkspaceReservationId::from_string("reservation-1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.writable_paths, created.writable_paths);
    assert_eq!(fetched.team_member_generation, 0);
    assert!(fetched.released_at.is_none());
}

#[tokio::test]
async fn test_overlapping_paths_conflict() {
    let (_db, repo) = setup_repo();

    repo.acquire(team_reservation("reservation-1", "team-1", "member-1"))
        .await
        .unwrap();

    // Exact match conflicts.
    let mut exact = team_reservation("reservation-2", "team-1", "member-2");
    exact.writable_paths = vec!["src/module_a".to_string()];
    assert!(repo.acquire(exact).await.is_err());

    // Nested path conflicts.
    let mut nested = team_reservation("reservation-3", "team-1", "member-2");
    nested.writable_paths = vec!["src/module_a/inner.rs".to_string()];
    assert!(repo.acquire(nested).await.is_err());

    // Sibling prefix does not conflict.
    let mut sibling = team_reservation("reservation-4", "team-1", "member-2");
    sibling.writable_paths = vec!["src/module_ab".to_string()];
    repo.acquire(sibling).await.unwrap();

    // Same path on another team does not conflict.
    let other_team = {
        let mut reservation = team_reservation("reservation-5", "team-2", "member-9");
        reservation.writable_paths = vec!["src/module_a".to_string()];
        reservation
    };
    repo.acquire(other_team).await.unwrap();
}

#[tokio::test]
async fn test_generated_outputs_and_resource_locks_conflict() {
    let (_db, repo) = setup_repo();

    let mut first = team_reservation("reservation-1", "team-1", "member-1");
    first.generated_outputs = vec!["dist/bundle".to_string()];
    first.resource_locks = vec!["cargo-target".to_string()];
    repo.acquire(first).await.unwrap();

    let mut output_overlap = team_reservation("reservation-2", "team-1", "member-2");
    output_overlap.writable_paths = vec!["dist/bundle/app.js".to_string()];
    assert!(repo.acquire(output_overlap).await.is_err());

    let mut lock_overlap = team_reservation("reservation-3", "team-1", "member-2");
    lock_overlap.writable_paths = vec!["src/other".to_string()];
    lock_overlap.resource_locks = vec!["cargo-target".to_string()];
    assert!(repo.acquire(lock_overlap).await.is_err());
}

#[tokio::test]
async fn test_release_frees_paths_and_guards_identity() {
    let (_db, repo) = setup_repo();
    let id = TeamWorkspaceReservationId::from_string("reservation-1");

    repo.acquire(team_reservation("reservation-1", "team-1", "member-1"))
        .await
        .unwrap();

    assert!(
        !repo.release_if_current(&id, 1, 1).await.unwrap(),
        "wrong generation must not release"
    );
    assert!(
        !repo.release_if_current(&id, 0, 2).await.unwrap(),
        "wrong attempt must not release"
    );
    assert!(repo.release_if_current(&id, 0, 1).await.unwrap());
    assert!(
        !repo.release_if_current(&id, 0, 1).await.unwrap(),
        "already-released reservation must not release again"
    );

    // Paths are free once released.
    repo.acquire(team_reservation("reservation-2", "team-1", "member-2"))
        .await
        .unwrap();
}

#[tokio::test]
async fn test_list_active_for_assignment() {
    let (_db, repo) = setup_repo();

    let mut assigned = team_reservation("reservation-1", "team-1", "member-1");
    assigned.assignment_id = Some(AgentTaskAssignmentId::from_string("assignment-1"));
    repo.acquire(assigned).await.unwrap();

    let mut other = team_reservation("reservation-2", "team-1", "member-2");
    other.writable_paths = vec!["src/other".to_string()];
    other.assignment_id = Some(AgentTaskAssignmentId::from_string("assignment-2"));
    repo.acquire(other).await.unwrap();

    let active = repo
        .list_active_for_assignment("assignment-1")
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id.0, "reservation-1");

    assert!(repo
        .release_if_current(
            &TeamWorkspaceReservationId::from_string("reservation-1"),
            0,
            1
        )
        .await
        .unwrap());
    assert!(repo
        .list_active_for_assignment("assignment-1")
        .await
        .unwrap()
        .is_empty());
}
