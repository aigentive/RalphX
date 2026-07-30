use crate::domain::entities::{TeamMessageDeliveryId, TeamWakeBatchId, TeamWakeBatchStatus};
use crate::domain::repositories::TeamWakeBatchRepository;
use crate::infrastructure::sqlite::SqliteTeamWakeBatchRepository;
use crate::testing::team_fixtures::{fixed_time, seed_team_session_row, team_wake_batch};
use crate::testing::SqliteTestDb;

fn setup_repo() -> (SqliteTestDb, SqliteTeamWakeBatchRepository) {
    let db = SqliteTestDb::new("sqlite-team-wake-batch-repo");
    db.with_connection(|conn| {
        seed_team_session_row(conn, "team-1", 101);
    });
    let repo = SqliteTeamWakeBatchRepository::from_shared(db.shared_conn());
    (db, repo)
}

#[tokio::test]
async fn test_create_then_extend_active_batch_for_recipient() {
    let (db, repo) = setup_repo();

    let created = repo
        .create_or_extend_active(team_wake_batch("batch-1", "team-1", "member-1"))
        .await
        .unwrap();
    assert_eq!(created.id.0, "batch-1");

    let mut extension = team_wake_batch("batch-2", "team-1", "member-1");
    extension.first_message_sequence = 2;
    extension.last_message_sequence = 5;
    extension.delivery_ids = vec![
        TeamMessageDeliveryId::from_string("delivery-1"),
        TeamMessageDeliveryId::from_string("delivery-9"),
    ];
    let extended = repo.create_or_extend_active(extension).await.unwrap();

    assert_eq!(
        extended.id.0, "batch-1",
        "an active batch for the recipient must be extended, not duplicated"
    );
    assert_eq!(extended.first_message_sequence, 1);
    assert_eq!(extended.last_message_sequence, 5);
    assert_eq!(
        extended
            .delivery_ids
            .iter()
            .map(|id| id.0.as_str())
            .collect::<Vec<_>>(),
        vec!["delivery-1", "delivery-9"]
    );

    let count: i64 = db.with_connection(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM managed_team_wake_batches WHERE team_id = 'team-1'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    });
    assert_eq!(count, 1, "one active wake batch per recipient/generation");
}

#[tokio::test]
async fn test_distinct_recipients_get_distinct_batches() {
    let (_db, repo) = setup_repo();

    let first = repo
        .create_or_extend_active(team_wake_batch("batch-1", "team-1", "member-1"))
        .await
        .unwrap();
    let second = repo
        .create_or_extend_active(team_wake_batch("batch-2", "team-1", "member-2"))
        .await
        .unwrap();
    assert_ne!(first.id.0, second.id.0);
}

#[tokio::test]
async fn test_settled_batch_allows_new_active_batch() {
    let (_db, repo) = setup_repo();
    let id = TeamWakeBatchId::from_string("batch-1");

    let mut batch = repo
        .create_or_extend_active(team_wake_batch("batch-1", "team-1", "member-1"))
        .await
        .unwrap();
    batch
        .transition_to(TeamWakeBatchStatus::Cancelled, fixed_time())
        .unwrap();
    batch.version = 1;
    assert!(repo
        .transition(&id, 0, TeamWakeBatchStatus::Queued, batch)
        .await
        .unwrap());

    let replacement = repo
        .create_or_extend_active(team_wake_batch("batch-2", "team-1", "member-1"))
        .await
        .unwrap();
    assert_eq!(replacement.id.0, "batch-2");
}

#[tokio::test]
async fn test_transition_cas_requires_version_and_status() {
    let (_db, repo) = setup_repo();
    let id = TeamWakeBatchId::from_string("batch-1");

    let mut batch = repo
        .create_or_extend_active(team_wake_batch("batch-1", "team-1", "member-1"))
        .await
        .unwrap();
    batch
        .transition_to(TeamWakeBatchStatus::Launching, fixed_time())
        .unwrap();
    batch.version = 1;

    assert!(
        !repo
            .transition(&id, 5, TeamWakeBatchStatus::Queued, batch.clone())
            .await
            .unwrap(),
        "stale version must not transition"
    );
    assert!(
        !repo
            .transition(&id, 0, TeamWakeBatchStatus::Running, batch.clone())
            .await
            .unwrap(),
        "wrong expected status must not transition"
    );
    assert!(repo
        .transition(&id, 0, TeamWakeBatchStatus::Queued, batch)
        .await
        .unwrap());

    let stored = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(stored.status, TeamWakeBatchStatus::Launching);
    assert_eq!(stored.version, 1);
}
