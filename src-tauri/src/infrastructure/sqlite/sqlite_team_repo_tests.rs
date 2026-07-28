use crate::domain::entities::{TeamMemberId, TeamSessionId, TeamSessionStatus};
use crate::domain::repositories::TeamRepository;
use crate::infrastructure::sqlite::SqliteTeamRepository;
use crate::testing::team_fixtures::{team_conversation_id, team_member, team_session};
use crate::testing::SqliteTestDb;

fn setup_repo() -> (SqliteTestDb, SqliteTeamRepository) {
    let db = SqliteTestDb::new("sqlite-team-repo");
    let repo = SqliteTeamRepository::from_shared(db.shared_conn());
    (db, repo)
}

#[tokio::test]
async fn test_ensure_session_creates_then_returns_existing_open_session() {
    let (_db, repo) = setup_repo();

    let created = repo
        .ensure_session(team_session("team-1", &team_conversation_id(1)))
        .await
        .unwrap();
    assert_eq!(created.id.as_str(), "team-1");

    let ensured = repo
        .ensure_session(team_session("team-2", &team_conversation_id(1)))
        .await
        .unwrap();
    assert_eq!(
        ensured.id.as_str(),
        "team-1",
        "second ensure for the same conversation must return the existing open session"
    );

    let open = repo
        .get_open_session_for_conversation(&team_conversation_id(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(open.id.as_str(), "team-1");
}

#[tokio::test]
async fn test_ensure_session_after_close_creates_new_session() {
    let (_db, repo) = setup_repo();

    let mut session = repo
        .ensure_session(team_session("team-1", &team_conversation_id(1)))
        .await
        .unwrap();
    session.status = TeamSessionStatus::Closed;
    session.version += 1;
    assert!(repo.update_session(session, 0).await.unwrap());

    let replacement = repo
        .ensure_session(team_session("team-2", &team_conversation_id(1)))
        .await
        .unwrap();
    assert_eq!(replacement.id.as_str(), "team-2");
}

#[tokio::test]
async fn test_update_session_rejects_stale_version() {
    let (_db, repo) = setup_repo();

    let mut session = repo
        .ensure_session(team_session("team-1", &team_conversation_id(1)))
        .await
        .unwrap();
    session.version = 1;
    assert!(repo.update_session(session.clone(), 0).await.unwrap());
    assert!(
        !repo.update_session(session, 0).await.unwrap(),
        "stale expected_version must not update"
    );

    let current = repo
        .get_session(&TeamSessionId::from_string("team-1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.version, 1);
}

#[tokio::test]
async fn test_member_names_unique_per_team() {
    let (_db, repo) = setup_repo();
    repo.ensure_session(team_session("team-1", &team_conversation_id(1)))
        .await
        .unwrap();
    repo.ensure_session(team_session("team-2", &team_conversation_id(2)))
        .await
        .unwrap();

    repo.create_member(team_member("member-1", "team-1", "researcher"))
        .await
        .unwrap();
    assert!(
        repo.create_member(team_member("member-2", "team-1", "researcher"))
            .await
            .is_err(),
        "duplicate normalized name in the same team must fail"
    );
    repo.create_member(team_member("member-3", "team-2", "researcher"))
        .await
        .unwrap();
}

#[tokio::test]
async fn test_member_roundtrip_and_generation_cas() {
    let (_db, repo) = setup_repo();
    repo.ensure_session(team_session("team-1", &team_conversation_id(1)))
        .await
        .unwrap();

    let created = repo
        .create_member(team_member("member-1", "team-1", "builder"))
        .await
        .unwrap();
    let fetched = repo
        .get_member(&TeamMemberId::from_string("member-1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.name, created.name);
    assert_eq!(fetched.status, created.status);
    assert_eq!(fetched.created_at, created.created_at);

    let mut updated = fetched.clone();
    updated.generation = 1;
    assert!(repo.update_member(updated.clone(), 0).await.unwrap());
    assert!(
        !repo.update_member(updated, 0).await.unwrap(),
        "stale expected_generation must not update"
    );

    let members = repo
        .list_members(&TeamSessionId::from_string("team-1"))
        .await
        .unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].generation, 1);
}

#[tokio::test]
async fn test_corrupt_stored_status_fails_closed() {
    let (db, repo) = setup_repo();
    repo.ensure_session(team_session("team-1", &team_conversation_id(1)))
        .await
        .unwrap();
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE managed_team_sessions SET status = 'not_a_status' WHERE id = 'team-1'",
            [],
        )
        .unwrap();
    });

    assert!(
        repo.get_session(&TeamSessionId::from_string("team-1"))
            .await
            .is_err(),
        "malformed stored status must surface an error, not None"
    );
}
