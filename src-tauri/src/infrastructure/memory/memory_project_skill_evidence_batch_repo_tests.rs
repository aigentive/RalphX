use chrono::{Duration, Utc};

use super::MemoryProjectSkillEvidenceBatchRepository;
use crate::domain::entities::{
    ProjectId, ProjectSkillEvidenceBatch, ProjectSkillEvidenceBatchId,
    ProjectSkillEvidenceBatchItem, ProjectSkillEvidenceBatchStatus, ProjectSkillId, TaskOutcomeId,
};
use crate::domain::repositories::ProjectSkillEvidenceBatchRepository;

fn batch(id: &str, outcome_id: &str, created_offset: i64) -> ProjectSkillEvidenceBatch {
    let now = Utc::now() + Duration::seconds(created_offset);
    ProjectSkillEvidenceBatch {
        id: ProjectSkillEvidenceBatchId::from_string(id),
        project_id: ProjectId::from_string("project-1".to_string()),
        fingerprint: format!("{:0>64}", id.as_bytes()[id.len() - 1]),
        bucket: "execution".to_string(),
        status: ProjectSkillEvidenceBatchStatus::Pending,
        claim_token: None,
        claimed_at: None,
        completed_project_skill_id: None,
        resolution_action: None,
        completed_at: None,
        created_at: now,
        updated_at: now,
        items: vec![ProjectSkillEvidenceBatchItem {
            outcome_id: TaskOutcomeId::from_string(outcome_id),
            ordinal: 0,
            digest: "bounded digest".to_string(),
        }],
    }
}

#[tokio::test]
async fn memory_batch_insert_is_idempotent_and_membership_is_unique() {
    let repository = MemoryProjectSkillEvidenceBatchRepository::new();
    let first = batch("batch-1", "outcome-1", 0);
    let saved = repository.insert_if_absent(first.clone()).await.unwrap();
    let duplicate = repository.insert_if_absent(first).await.unwrap();
    assert_eq!(saved.id, duplicate.id);

    let conflict = repository
        .insert_if_absent(batch("batch-2", "outcome-1", 1))
        .await;
    assert!(conflict.is_err());
    assert_eq!(
        repository
            .list_batched_outcome_ids(&ProjectId::from_string("project-1".to_string()))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn memory_claim_is_exclusive_and_wrong_token_cannot_release_or_complete() {
    let repository = MemoryProjectSkillEvidenceBatchRepository::new();
    repository
        .insert_if_absent(batch("batch-1", "outcome-1", 0))
        .await
        .unwrap();

    let claimed_at = Utc::now();
    let claimed = repository
        .claim_oldest_pending(
            &ProjectId::from_string("project-1".to_string()),
            "winner",
            claimed_at,
        )
        .await
        .unwrap()
        .unwrap();
    assert!(repository
        .claim_oldest_pending(
            &ProjectId::from_string("project-1".to_string()),
            "loser",
            claimed_at,
        )
        .await
        .unwrap()
        .is_none());
    assert!(!repository
        .release_claim(&claimed.id, "loser", Utc::now())
        .await
        .unwrap());
    assert!(!repository
        .complete_claim(
            &claimed.id,
            "loser",
            &ProjectId::from_string("project-1".to_string()),
            &ProjectSkillId::from_string("skill-1"),
            "create_new",
            Utc::now(),
        )
        .await
        .unwrap());
}

#[tokio::test]
async fn memory_stale_recovery_requeues_only_uncompleted_claims() {
    let repository = MemoryProjectSkillEvidenceBatchRepository::new();
    repository
        .insert_if_absent(batch("batch-1", "outcome-1", 0))
        .await
        .unwrap();
    let claimed = repository
        .claim_oldest_pending(
            &ProjectId::from_string("project-1".to_string()),
            "claim",
            Utc::now() - Duration::hours(1),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        repository
            .requeue_stale_claims(
                &ProjectId::from_string("project-1".to_string()),
                Utc::now() - Duration::minutes(30),
                Utc::now(),
            )
            .await
            .unwrap(),
        1
    );
    let reclaimed = repository
        .claim_oldest_pending(
            &ProjectId::from_string("project-1".to_string()),
            "claim-2",
            Utc::now() - Duration::hours(1),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed.id, claimed.id);
    assert!(repository
        .complete_claim(
            &reclaimed.id,
            "claim-2",
            &ProjectId::from_string("project-1".to_string()),
            &ProjectSkillId::from_string("skill-1"),
            "duplicate",
            Utc::now() - Duration::minutes(31),
        )
        .await
        .unwrap());
    assert_eq!(
        repository
            .requeue_stale_claims(
                &ProjectId::from_string("project-1".to_string()),
                Utc::now() - Duration::minutes(30),
                Utc::now(),
            )
            .await
            .unwrap(),
        0
    );
}
