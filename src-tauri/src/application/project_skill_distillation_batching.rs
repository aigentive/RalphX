use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::domain::entities::{
    ProjectId, ProjectSkillEvidenceBatch, ProjectSkillEvidenceBatchId,
    ProjectSkillEvidenceBatchItem, ProjectSkillEvidenceBatchStatus, TaskOutcome, TaskOutcomeSource,
    PROJECT_SKILL_EVIDENCE_DIGEST_MAX_CHARS,
};

pub(super) fn build_batch(
    project_id: &ProjectId,
    bucket: &str,
    outcomes: &[TaskOutcome],
) -> ProjectSkillEvidenceBatch {
    let items = outcomes
        .iter()
        .enumerate()
        .map(|(ordinal, outcome)| ProjectSkillEvidenceBatchItem {
            outcome_id: outcome.id.clone(),
            ordinal,
            digest: outcome_digest(outcome),
        })
        .collect::<Vec<_>>();
    let canonical = serde_json::json!({
        "bucket": bucket,
        "items": items.iter().map(|item| serde_json::json!({
            "outcome_id": item.outcome_id.as_str(),
            "digest": item.digest,
        })).collect::<Vec<_>>(),
    });
    let fingerprint = outcomes
        .first()
        .filter(|_| outcomes.len() == 1)
        .and_then(verification_gap_fingerprint)
        .map(str::to_string)
        .unwrap_or_else(|| format!("{:x}", Sha256::digest(canonical.to_string().as_bytes())));
    let now = Utc::now();
    ProjectSkillEvidenceBatch {
        id: ProjectSkillEvidenceBatchId::new(),
        project_id: project_id.clone(),
        fingerprint,
        bucket: bucket.to_string(),
        status: ProjectSkillEvidenceBatchStatus::Pending,
        claim_token: None,
        claimed_at: None,
        completed_project_skill_id: None,
        resolution_action: None,
        completed_at: None,
        created_at: now,
        updated_at: now,
        items,
    }
}

pub(super) fn verification_gap_fingerprint(outcome: &TaskOutcome) -> Option<&str> {
    if outcome.source != TaskOutcomeSource::Verification
        || outcome.source_ref_kind != "gap_recurrence"
    {
        return None;
    }
    outcome
        .evidence_json
        .get("fingerprint")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|fingerprint| {
            fingerprint.len() == 64
                && fingerprint
                    .bytes()
                    .all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase())
        })
}

pub(super) fn bucket_for_outcome_source(source: TaskOutcomeSource) -> &'static str {
    match source {
        TaskOutcomeSource::Verification => "verification",
        TaskOutcomeSource::Review => "review",
        TaskOutcomeSource::Merge => "merge",
        TaskOutcomeSource::PlanMode => "planning",
        _ => "execution",
    }
}

fn outcome_digest(outcome: &TaskOutcome) -> String {
    let canonical = serde_json::json!({
        "source": outcome.source,
        "source_ref_kind": outcome.source_ref_kind,
        "source_ref_id": outcome.source_ref_id,
        "outcome_class": outcome.outcome_class,
        "task_id": outcome.task_id,
        "conversation_id": outcome.conversation_id,
        "agent_run_id": outcome.agent_run_id,
        "pull_request_id": outcome.pull_request_id,
        "proposal_id": outcome.proposal_id,
        "verification_id": outcome.verification_id,
        "review_id": outcome.review_id,
        "evidence": outcome.evidence_json,
    })
    .to_string();
    let prefix = verification_gap_fingerprint(outcome)
        .map(|fingerprint| format!("verification_gap_fingerprint={fingerprint}\n"))
        .unwrap_or_default();
    let remaining = PROJECT_SKILL_EVIDENCE_DIGEST_MAX_CHARS.saturating_sub(prefix.chars().count());
    prefix + &canonical.chars().take(remaining).collect::<String>()
}
