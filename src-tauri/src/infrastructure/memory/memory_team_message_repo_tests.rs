use crate::domain::entities::{TeamMessageDeliveryStatus, TeamMessageId, TeamSessionId};
use crate::domain::repositories::TeamMessageRepository;
use crate::infrastructure::memory::MemoryTeamMessageRepository;
use crate::testing::team_fixtures::{fixed_time, team_delivery, team_message};

#[tokio::test]
async fn test_duplicate_sequence_or_idempotency_key_rejected() {
    let repo = MemoryTeamMessageRepository::new();

    repo.create_envelope_with_deliveries(
        team_message("message-1", "team-1", 1),
        vec![team_delivery("delivery-1", "message-1", None)],
    )
    .await
    .unwrap();

    assert!(repo
        .create_envelope_with_deliveries(
            team_message("message-2", "team-1", 1),
            vec![team_delivery("delivery-2", "message-2", None)],
        )
        .await
        .is_err());

    let mut duplicate_idem = team_message("message-3", "team-1", 2);
    duplicate_idem.idempotency_key = "idem-message-1".to_string();
    assert!(repo
        .create_envelope_with_deliveries(
            duplicate_idem,
            vec![team_delivery("delivery-3", "message-3", None)],
        )
        .await
        .is_err());

    repo.create_envelope_with_deliveries(
        team_message("message-4", "team-2", 1),
        vec![team_delivery("delivery-4", "message-4", None)],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn test_fan_out_rejects_duplicate_delivery_and_mismatched_envelope() {
    let repo = MemoryTeamMessageRepository::new();

    repo.create_envelope_with_deliveries(
        team_message("message-1", "team-1", 1),
        vec![team_delivery("delivery-1", "message-1", Some("member-1"))],
    )
    .await
    .unwrap();

    assert!(repo
        .create_envelope_with_deliveries(
            team_message("message-2", "team-1", 2),
            vec![team_delivery("delivery-1", "message-2", Some("member-2"))],
        )
        .await
        .is_err());
    assert!(
        repo.get_message(&TeamMessageId::from_string("message-2"))
            .await
            .unwrap()
            .is_none(),
        "rejected fan-out must not store the envelope"
    );

    assert!(repo
        .create_envelope_with_deliveries(
            team_message("message-3", "team-1", 3),
            vec![team_delivery("delivery-9", "message-other", None)],
        )
        .await
        .is_err());
}

#[tokio::test]
async fn test_list_messages_after_and_transition_delivery() {
    let repo = MemoryTeamMessageRepository::new();
    for sequence in 1..=3 {
        repo.create_envelope_with_deliveries(
            team_message(&format!("message-{sequence}"), "team-1", sequence),
            vec![team_delivery(
                &format!("delivery-{sequence}"),
                &format!("message-{sequence}"),
                Some("member-1"),
            )],
        )
        .await
        .unwrap();
    }

    let messages = repo
        .list_messages_after(&TeamSessionId::from_string("team-1"), 1, 1)
        .await
        .unwrap();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.sequence)
            .collect::<Vec<_>>(),
        vec![2]
    );

    let actionable = repo
        .list_actionable_deliveries(&TeamSessionId::from_string("team-1"), 10)
        .await
        .unwrap();
    assert_eq!(actionable.len(), 3);

    let mut updated = actionable[0].clone();
    let id = updated.id.clone();
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
    assert!(!repo
        .transition_delivery(
            &id,
            TeamMessageDeliveryStatus::Pending,
            TeamMessageDeliveryStatus::Queued,
            updated,
        )
        .await
        .unwrap());
}
