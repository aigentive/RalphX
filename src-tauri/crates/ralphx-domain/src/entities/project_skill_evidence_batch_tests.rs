use chrono::Utc;

use super::project_skill_evidence_batch::{
    ProjectSkillEvidenceBatch, ProjectSkillEvidenceBatchId, ProjectSkillEvidenceBatchItem,
    ProjectSkillEvidenceBatchStatus, PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS,
    PROJECT_SKILL_EVIDENCE_DIGEST_MAX_CHARS,
};
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::TaskOutcomeId;

fn batch_with_items(count: usize, digest: &str) -> ProjectSkillEvidenceBatch {
    let now = Utc::now();
    ProjectSkillEvidenceBatch {
        id: ProjectSkillEvidenceBatchId::from_string("batch-1"),
        project_id: ProjectId::from_string("project-1".to_string()),
        fingerprint: "a".repeat(64),
        bucket: "execution".to_string(),
        status: ProjectSkillEvidenceBatchStatus::Pending,
        claim_token: None,
        claimed_at: None,
        completed_project_skill_id: None,
        resolution_action: None,
        completed_at: None,
        created_at: now,
        updated_at: now,
        items: (0..count)
            .map(|ordinal| ProjectSkillEvidenceBatchItem {
                outcome_id: TaskOutcomeId::from_string(format!("outcome-{ordinal}")),
                ordinal,
                digest: digest.to_string(),
            })
            .collect(),
    }
}

#[test]
fn evidence_batch_validation_accepts_exact_item_and_unicode_digest_bounds() {
    let digest = "🦀".repeat(PROJECT_SKILL_EVIDENCE_DIGEST_MAX_CHARS);
    let batch = batch_with_items(PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS, &digest);

    assert!(batch.validate_for_insert().is_ok());
}

#[test]
fn evidence_batch_validation_rejects_empty_oversized_and_duplicate_membership() {
    assert!(batch_with_items(0, "digest").validate_for_insert().is_err());
    assert!(
        batch_with_items(PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS + 1, "digest")
            .validate_for_insert()
            .is_err()
    );
    assert!(
        batch_with_items(1, &"x".repeat(PROJECT_SKILL_EVIDENCE_DIGEST_MAX_CHARS + 1))
            .validate_for_insert()
            .is_err()
    );

    let mut duplicate = batch_with_items(2, "digest");
    duplicate.items[1].outcome_id = duplicate.items[0].outcome_id.clone();
    assert!(duplicate.validate_for_insert().is_err());
}

#[test]
fn evidence_batch_validation_rejects_noncanonical_fingerprint_and_state() {
    let mut invalid_fingerprint = batch_with_items(1, "digest");
    invalid_fingerprint.fingerprint = "not-sha256".to_string();
    assert!(invalid_fingerprint.validate_for_insert().is_err());

    let mut claimed = batch_with_items(1, "digest");
    claimed.status = ProjectSkillEvidenceBatchStatus::Consumed;
    claimed.claim_token = Some("claim".to_string());
    claimed.claimed_at = Some(Utc::now());
    assert!(claimed.validate_for_insert().is_err());
}
