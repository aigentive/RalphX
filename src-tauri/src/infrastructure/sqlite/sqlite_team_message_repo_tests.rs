use crate::domain::entities::{
    TeamMessageDeliveryId, TeamMessageDeliveryStatus, TeamMessageId, TeamSessionId,
};
use crate::domain::repositories::TeamMessageRepository;
use crate::infrastructure::sqlite::SqliteTeamMessageRepository;
use crate::testing::team_fixtures::{
    fixed_time, seed_team_session_row, team_delivery, team_message,
};
use crate::testing::SqliteTestDb;

fn setup_repo() -> (SqliteTestDb, SqliteTeamMessageRepository) {
    let db = SqliteTestDb::new("sqlite-team-message-repo");
    db.with_connection(|conn| {
        seed_team_session_row(conn, "team-1", 101);
        seed_team_session_row(conn, "team-2", 102);
    });
    let repo = SqliteTeamMessageRepository::from_shared(db.shared_conn());
    (db, repo)
}

#[tokio::test]
async fn test_envelope_with_deliveries_roundtrip() {
    let (_db, repo) = setup_repo();

    let (message, deliveries) = repo
        .create_envelope_with_deliveries(
            team_message("message-1", "team-1", 1),
            vec![
                team_delivery("delivery-1", "message-1", Some("member-1")),
                team_delivery("delivery-2", "message-1", None),
            ],
        )
        .await
        .unwrap();
    assert_eq!(deliveries.len(), 2);

    let stored = repo
        .get_message(&TeamMessageId::from_string("message-1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.sequence, message.sequence);
    assert_eq!(stored.content, message.content);

    let actionable = repo
        .list_actionable_deliveries(&TeamSessionId::from_string("team-1"), 10)
        .await
        .unwrap();
    assert_eq!(actionable.len(), 2);
}

#[tokio::test]
async fn test_envelope_fan_out_is_atomic() {
    let (_db, repo) = setup_repo();

    repo.create_envelope_with_deliveries(
        team_message("message-1", "team-1", 1),
        vec![team_delivery("delivery-1", "message-1", Some("member-1"))],
    )
    .await
    .unwrap();

    let result = repo
        .create_envelope_with_deliveries(
            team_message("message-2", "team-1", 2),
            vec![
                team_delivery("delivery-2", "message-2", Some("member-1")),
                // Duplicate delivery id: second insert fails after the first succeeded.
                team_delivery("delivery-1", "message-2", Some("member-2")),
            ],
        )
        .await;
    assert!(result.is_err());

    assert!(
        repo.get_message(&TeamMessageId::from_string("message-2"))
            .await
            .unwrap()
            .is_none(),
        "failed fan-out must roll back the envelope"
    );
    let actionable = repo
        .list_actionable_deliveries(&TeamSessionId::from_string("team-1"), 10)
        .await
        .unwrap();
    assert_eq!(
        actionable.len(),
        1,
        "failed fan-out must not leave partial deliveries"
    );
}

#[tokio::test]
async fn test_delivery_envelope_mismatch_is_rejected() {
    let (_db, repo) = setup_repo();

    let result = repo
        .create_envelope_with_deliveries(
            team_message("message-1", "team-1", 1),
            vec![team_delivery(
                "delivery-1",
                "message-other",
                Some("member-1"),
            )],
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_sequence_and_idempotency_unique_per_team() {
    let (_db, repo) = setup_repo();

    repo.create_envelope_with_deliveries(
        team_message("message-1", "team-1", 1),
        vec![team_delivery("delivery-1", "message-1", None)],
    )
    .await
    .unwrap();

    assert!(
        repo.create_envelope_with_deliveries(
            team_message("message-2", "team-1", 1),
            vec![team_delivery("delivery-2", "message-2", None)],
        )
        .await
        .is_err(),
        "duplicate sequence in the same team must fail"
    );

    let mut duplicate_idem = team_message("message-3", "team-1", 2);
    duplicate_idem.idempotency_key = "idem-message-1".to_string();
    assert!(
        repo.create_envelope_with_deliveries(
            duplicate_idem,
            vec![team_delivery("delivery-3", "message-3", None)],
        )
        .await
        .is_err(),
        "duplicate idempotency key in the same team must fail"
    );

    // Same sequence in a different team is allowed.
    repo.create_envelope_with_deliveries(
        team_message("message-4", "team-2", 1),
        vec![team_delivery("delivery-4", "message-4", None)],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn test_list_messages_after_orders_and_limits() {
    let (_db, repo) = setup_repo();
    for sequence in 1..=4 {
        repo.create_envelope_with_deliveries(
            team_message(&format!("message-{sequence}"), "team-1", sequence),
            vec![team_delivery(
                &format!("delivery-{sequence}"),
                &format!("message-{sequence}"),
                None,
            )],
        )
        .await
        .unwrap();
    }

    let messages = repo
        .list_messages_after(&TeamSessionId::from_string("team-1"), 1, 2)
        .await
        .unwrap();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.sequence)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
}

#[tokio::test]
async fn test_transition_delivery_cas() {
    let (_db, repo) = setup_repo();
    let id = TeamMessageDeliveryId::from_string("delivery-1");

    let (_, deliveries) = repo
        .create_envelope_with_deliveries(
            team_message("message-1", "team-1", 1),
            vec![team_delivery("delivery-1", "message-1", Some("member-1"))],
        )
        .await
        .unwrap();

    let mut updated = deliveries[0].clone();
    updated
        .transition_to(TeamMessageDeliveryStatus::Queued, fixed_time())
        .unwrap();

    assert!(repo
        .transition_delivery(
            &id,
            TeamMessageDeliveryStatus::Pending,
            TeamMessageDeliveryStatus::Queued,
            updated.clone(),
        )
        .await
        .unwrap());
    assert!(
        !repo
            .transition_delivery(
                &id,
                TeamMessageDeliveryStatus::Pending,
                TeamMessageDeliveryStatus::Queued,
                updated.clone(),
            )
            .await
            .unwrap(),
        "stale expected status must not transition again"
    );
    assert!(
        !repo
            .transition_delivery(
                &id,
                TeamMessageDeliveryStatus::Queued,
                TeamMessageDeliveryStatus::Delivered,
                updated,
            )
            .await
            .unwrap(),
        "payload status must match the requested next status"
    );

    let actionable = repo
        .list_actionable_deliveries(&TeamSessionId::from_string("team-1"), 10)
        .await
        .unwrap();
    assert!(
        actionable.is_empty(),
        "queued deliveries are no longer actionable"
    );

    let fetched = repo
        .get_delivery(&id)
        .await
        .unwrap()
        .expect("delivery exists");
    assert_eq!(fetched.status, TeamMessageDeliveryStatus::Queued);
    assert!(repo
        .get_delivery(&TeamMessageDeliveryId::from_string("missing"))
        .await
        .unwrap()
        .is_none());
}
