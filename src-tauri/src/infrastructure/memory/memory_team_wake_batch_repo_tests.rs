use crate::domain::entities::{TeamMessageDeliveryId, TeamWakeBatchId, TeamWakeBatchStatus};
use crate::domain::repositories::TeamWakeBatchRepository;
use crate::infrastructure::memory::MemoryTeamWakeBatchRepository;
use crate::testing::team_fixtures::{fixed_time, team_wake_batch};

#[tokio::test]
async fn test_create_then_extend_active_batch_for_recipient() {
    let repo = MemoryTeamWakeBatchRepository::new();

    repo.create_or_extend_active(team_wake_batch("batch-1", "team-1", "member-1"))
        .await
        .unwrap();

    let mut extension = team_wake_batch("batch-2", "team-1", "member-1");
    extension.first_message_sequence = 2;
    extension.last_message_sequence = 5;
    extension.delivery_ids = vec![
        TeamMessageDeliveryId::from_string("delivery-1"),
        TeamMessageDeliveryId::from_string("delivery-9"),
    ];
    let extended = repo.create_or_extend_active(extension).await.unwrap();

    assert_eq!(extended.id.0, "batch-1");
    assert_eq!(extended.first_message_sequence, 1);
    assert_eq!(extended.last_message_sequence, 5);
    assert_eq!(extended.delivery_ids.len(), 2);

    assert!(repo
        .get_by_id(&TeamWakeBatchId::from_string("batch-2"))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_settled_batch_allows_new_active_batch_and_transition_cas() {
    let repo = MemoryTeamWakeBatchRepository::new();
    let id = TeamWakeBatchId::from_string("batch-1");

    let mut batch = repo
        .create_or_extend_active(team_wake_batch("batch-1", "team-1", "member-1"))
        .await
        .unwrap();
    batch
        .transition_to(TeamWakeBatchStatus::Cancelled, fixed_time())
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

    let replacement = repo
        .create_or_extend_active(team_wake_batch("batch-2", "team-1", "member-1"))
        .await
        .unwrap();
    assert_eq!(replacement.id.0, "batch-2");
}
