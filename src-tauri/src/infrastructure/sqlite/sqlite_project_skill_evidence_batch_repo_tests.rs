use std::sync::Arc;

use chrono::{Duration, Utc};

use super::SqliteProjectSkillEvidenceBatchRepository;
use crate::domain::entities::{
    ProjectId, ProjectSkillEvidenceBatch, ProjectSkillEvidenceBatchId,
    ProjectSkillEvidenceBatchItem, ProjectSkillEvidenceBatchStatus, ProjectSkillId, TaskOutcomeId,
};
use crate::domain::repositories::ProjectSkillEvidenceBatchRepository;
use crate::testing::SqliteTestDb;

fn batch(id: &str, outcome_id: &str) -> ProjectSkillEvidenceBatch {
    let now = Utc::now();
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

fn setup(name: &str) -> (SqliteTestDb, Arc<SqliteProjectSkillEvidenceBatchRepository>) {
    let database = SqliteTestDb::new(name);
    database.with_connection(|connection| {
        connection
            .execute(
                "INSERT INTO projects (id, name, working_directory)
                 VALUES ('project-1', 'Project 1', '')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO task_outcomes (
                    id, project_id, source, source_ref_kind, source_ref_id,
                    outcome_class, status, evidence_json
                 ) VALUES (
                    'outcome-1', 'project-1', 'task_pipeline', 'task', 'task-1',
                    'success', 'eligible', '{}'
                 )",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO project_skills (
                    id, project_id, title, bucket, stage, status, compact_guidance,
                    body_markdown, predicted_effect
                 ) VALUES (
                    'skill-1', 'project-1', 'Skill', 'execution', 'execution', 'staged',
                    'guidance', 'body', 'effect'
                 )",
                [],
            )
            .unwrap();
    });
    let repository = Arc::new(SqliteProjectSkillEvidenceBatchRepository::from_shared(
        database.shared_conn(),
    ));
    (database, repository)
}

#[tokio::test]
async fn sqlite_insert_is_idempotent_and_rejects_cross_project_membership() {
    let (database, repository) = setup("evidence-batch-insert");
    let first = batch("batch-1", "outcome-1");
    let saved = repository.insert_if_absent(first.clone()).await.unwrap();
    let duplicate = repository.insert_if_absent(first).await.unwrap();
    assert_eq!(saved.id, duplicate.id);

    database.with_connection(|connection| {
        connection
            .execute(
                "INSERT INTO projects (id, name, working_directory)
                 VALUES ('project-2', 'Project 2', 'project-2')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO task_outcomes (
                    id, project_id, source, source_ref_kind, source_ref_id,
                    outcome_class, status, evidence_json
                 ) VALUES (
                    'outcome-2', 'project-2', 'task_pipeline', 'task', 'task-2',
                    'success', 'eligible', '{}'
                 )",
                [],
            )
            .unwrap();
    });
    assert!(repository
        .insert_if_absent(batch("batch-2", "outcome-2"))
        .await
        .is_err());
}

#[tokio::test]
async fn sqlite_claim_is_exclusive_and_settlement_is_token_scoped() {
    let (_database, repository) = setup("evidence-batch-claim");
    repository
        .insert_if_absent(batch("batch-1", "outcome-1"))
        .await
        .unwrap();
    let project_id = ProjectId::from_string("project-1".to_string());
    let claimed_at = Utc::now();
    let (first, second) = tokio::join!(
        repository.claim_oldest_pending(&project_id, "claim-1", claimed_at),
        repository.claim_oldest_pending(&project_id, "claim-2", claimed_at),
    );
    let claims = [first.unwrap(), second.unwrap()];
    assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
    let claimed = claims.into_iter().flatten().next().unwrap();
    let winner = claimed.claim_token.clone().unwrap();
    assert!(!repository
        .complete_claim(
            &claimed.id,
            "wrong",
            &project_id,
            &ProjectSkillId::from_string("skill-1"),
            "create_new",
            Utc::now(),
        )
        .await
        .unwrap());
    assert!(repository
        .complete_claim(
            &claimed.id,
            &winner,
            &project_id,
            &ProjectSkillId::from_string("skill-1"),
            "create_new",
            Utc::now(),
        )
        .await
        .unwrap());
    assert!(!repository
        .release_claim(&claimed.id, &winner, Utc::now())
        .await
        .unwrap());
}

#[tokio::test]
async fn sqlite_stale_recovery_preserves_completed_claim() {
    let (_database, repository) = setup("evidence-batch-stale");
    repository
        .insert_if_absent(batch("batch-1", "outcome-1"))
        .await
        .unwrap();
    let project_id = ProjectId::from_string("project-1".to_string());
    let claimed = repository
        .claim_oldest_pending(&project_id, "claim-1", Utc::now() - Duration::hours(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        repository
            .requeue_stale_claims(&project_id, Utc::now() - Duration::minutes(30), Utc::now(),)
            .await
            .unwrap(),
        1
    );
    let reclaimed = repository
        .claim_oldest_pending(&project_id, "claim-2", Utc::now() - Duration::hours(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.id, reclaimed.id);
    assert!(repository
        .complete_claim(
            &reclaimed.id,
            "claim-2",
            &project_id,
            &ProjectSkillId::from_string("skill-1"),
            "duplicate",
            Utc::now() - Duration::minutes(31),
        )
        .await
        .unwrap());
    assert_eq!(
        repository
            .requeue_stale_claims(&project_id, Utc::now() - Duration::minutes(30), Utc::now(),)
            .await
            .unwrap(),
        0
    );
}
