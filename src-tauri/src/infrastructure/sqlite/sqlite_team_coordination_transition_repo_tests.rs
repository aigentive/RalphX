use crate::domain::entities::{CoordinationMode, TeamSessionId};
use crate::domain::repositories::{TeamCoordinationTransitionRepository, TeamExitMarker};
use crate::infrastructure::sqlite::SqliteTeamCoordinationTransitionRepository;
use crate::testing::team_fixtures::{team_conversation_id, team_session};
use crate::testing::SqliteTestDb;

fn setup_repo() -> (SqliteTestDb, SqliteTeamCoordinationTransitionRepository) {
    let db = SqliteTestDb::new("sqlite-team-coordination-repo");
    let repo = SqliteTeamCoordinationTransitionRepository::from_shared(db.shared_conn());
    (db, repo)
}

fn exit_marker() -> TeamExitMarker {
    TeamExitMarker {
        coordination_mode: CoordinationMode::Solo,
        exit_action: "suspend".to_string(),
    }
}

#[tokio::test]
async fn test_enter_team_is_idempotent_per_conversation() {
    let (_db, repo) = setup_repo();
    let conversation = team_conversation_id(1);

    let first = repo
        .enter_team(
            &conversation,
            team_session("team-1", &team_conversation_id(1)),
        )
        .await
        .unwrap();
    let second = repo
        .enter_team(
            &conversation,
            team_session("team-2", &team_conversation_id(1)),
        )
        .await
        .unwrap();
    assert_eq!(first.id.as_str(), "team-1");
    assert_eq!(
        second.id.as_str(),
        "team-1",
        "re-entry must return the existing open team, not create another"
    );
}

#[tokio::test]
async fn test_mark_pending_exit_and_commit_exit_cas() {
    let (_db, repo) = setup_repo();
    let conversation = team_conversation_id(1);
    let team_id = TeamSessionId::from_string("team-1");

    repo.enter_team(
        &conversation,
        team_session("team-1", &team_conversation_id(1)),
    )
    .await
    .unwrap();

    assert!(repo
        .mark_pending_exit(&team_id, 0, exit_marker())
        .await
        .unwrap());
    assert!(
        !repo
            .mark_pending_exit(&team_id, 0, exit_marker())
            .await
            .unwrap(),
        "stale version must not mark exit again"
    );

    assert!(
        !repo
            .commit_exit(&team_conversation_id(9), &team_id, 1)
            .await
            .unwrap(),
        "commit for a different conversation owner must fail"
    );
    assert!(repo.commit_exit(&conversation, &team_id, 1).await.unwrap());
    assert!(
        !repo.commit_exit(&conversation, &team_id, 1).await.unwrap(),
        "stale version must not commit twice"
    );
}
