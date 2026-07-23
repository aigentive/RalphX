use std::sync::Arc;

use chrono::{Duration, Utc};

use super::project_skill_distillation_batching::bucket_for_outcome_source;
use super::project_skill_distillation_service::{
    ProjectSkillDistillationSelection, ProjectSkillDistillationService,
    ProjectSkillDistillationTrigger,
};
use crate::domain::entities::{
    ProjectId, ProjectSkillSettings, TaskOutcome, TaskOutcomeClass, TaskOutcomeId,
    TaskOutcomeSource, TaskOutcomeStatus, PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS,
    PROJECT_SKILL_EVIDENCE_DIGEST_MAX_CHARS,
};
use crate::domain::repositories::{
    MemoryEventRepository, ProjectSkillEvidenceBatchRepository, ProjectSkillSettingsRepository,
    TaskOutcomeRepository, UpsertTaskOutcomeInput,
};
use crate::infrastructure::memory::{
    InMemoryMemoryEventRepository, MemoryProjectSkillEvidenceBatchRepository,
    MemoryProjectSkillRepository, MemoryProjectSkillSettingsRepository,
    MemoryTaskOutcomeRepository,
};

fn eligible_outcome(
    project_id: &ProjectId,
    id: usize,
    created_offset: i64,
    evidence_chars: usize,
) -> TaskOutcome {
    let created_at = Utc::now() + Duration::seconds(created_offset);
    TaskOutcome {
        id: TaskOutcomeId::from_string(format!("outcome-{id:02}")),
        project_id: project_id.clone(),
        source: TaskOutcomeSource::TaskPipeline,
        source_ref_kind: "task".to_string(),
        source_ref_id: format!("task-{id:02}"),
        task_id: Some(format!("task-{id:02}")),
        conversation_id: Some("conversation-1".to_string()),
        agent_run_id: Some(format!("run-{id:02}")),
        pull_request_id: None,
        proposal_id: None,
        verification_id: None,
        review_id: None,
        outcome_class: Some(TaskOutcomeClass::Other("completed".to_string())),
        status: TaskOutcomeStatus::Eligible,
        evidence_json: serde_json::json!({ "summary": "é".repeat(evidence_chars) }),
        failure_fingerprint: None,
        provider_harness: Some("codex".to_string()),
        provider_session_id: Some(format!("session-{id:02}")),
        created_at,
        updated_at: created_at,
    }
}

#[test]
fn typed_outcome_source_buckets_preserve_pre_d3_live_source_behavior() {
    assert_eq!(
        bucket_for_outcome_source(TaskOutcomeSource::Verification),
        "verification"
    );
    assert_eq!(
        bucket_for_outcome_source(TaskOutcomeSource::Review),
        "review"
    );
    assert_eq!(bucket_for_outcome_source(TaskOutcomeSource::Merge), "merge");
    assert_eq!(
        bucket_for_outcome_source(TaskOutcomeSource::PlanMode),
        "planning"
    );
    assert_eq!(
        bucket_for_outcome_source(TaskOutcomeSource::GithubPrReview),
        "execution"
    );
    assert_eq!(
        bucket_for_outcome_source(TaskOutcomeSource::MergeValidation),
        "execution"
    );
}

async fn service_fixture(
    project_id: &ProjectId,
) -> (
    ProjectSkillDistillationService,
    Arc<MemoryTaskOutcomeRepository>,
    Arc<MemoryProjectSkillEvidenceBatchRepository>,
    Arc<MemoryProjectSkillSettingsRepository>,
    Arc<InMemoryMemoryEventRepository>,
) {
    let outcomes = Arc::new(MemoryTaskOutcomeRepository::new());
    let batches = Arc::new(MemoryProjectSkillEvidenceBatchRepository::new());
    let settings = Arc::new(MemoryProjectSkillSettingsRepository::new());
    let skills = Arc::new(MemoryProjectSkillRepository::new());
    let events = Arc::new(InMemoryMemoryEventRepository::new());
    settings
        .upsert(ProjectSkillSettings::default_for_project(
            project_id.clone(),
        ))
        .await
        .expect("seed project skill settings");
    let service = ProjectSkillDistillationService::new(
        outcomes.clone(),
        batches.clone(),
        settings.clone(),
        skills,
        events.clone(),
    );
    (service, outcomes, batches, settings, events)
}

#[tokio::test]
async fn prepare_claim_groups_oldest_eligible_evidence_into_stable_bounded_batches() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let (service, outcomes, batches, _settings, _events) = service_fixture(&project_id).await;
    for id in (0..10).rev() {
        outcomes
            .upsert(UpsertTaskOutcomeInput {
                outcome: eligible_outcome(&project_id, id, id as i64, 2_000),
            })
            .await
            .expect("seed eligible outcome");
    }

    let first = service
        .prepare_claim(
            &project_id,
            ProjectSkillDistillationTrigger::Automatic,
            1_800,
        )
        .await
        .expect("prepare first claim")
        .expect("first claim exists");
    assert_eq!(
        first.batch.items.len(),
        PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS
    );
    assert_eq!(
        first
            .batch
            .items
            .iter()
            .map(|item| item.outcome_id.as_str())
            .collect::<Vec<_>>(),
        (0..8)
            .map(|id| format!("outcome-{id:02}"))
            .collect::<Vec<_>>()
    );
    assert!(first
        .batch
        .items
        .iter()
        .all(|item| { item.digest.chars().count() <= PROJECT_SKILL_EVIDENCE_DIGEST_MAX_CHARS }));
    assert!(first.prompt.contains(&first.batch.fingerprint));
    assert!(first.prompt.contains("upsert_project_skill"));
    assert!(first.prompt.contains("patch_project_skill"));
    assert!(first.prompt.contains("retire_project_skill"));
    assert!(!first.prompt.contains("claim_token"));

    let second = service
        .prepare_claim(
            &project_id,
            ProjectSkillDistillationTrigger::Automatic,
            1_800,
        )
        .await
        .expect("prepare second claim")
        .expect("second claim exists");
    assert_eq!(second.batch.items.len(), 2);
    assert_eq!(
        batches
            .list_batched_outcome_ids(&project_id)
            .await
            .expect("list batched outcomes")
            .len(),
        10
    );
    assert!(service
        .prepare_claim(
            &project_id,
            ProjectSkillDistillationTrigger::Automatic,
            1_800,
        )
        .await
        .expect("prepare exhausted claim")
        .is_none());
}

#[tokio::test]
async fn automatic_gate_is_durable_and_explicit_trigger_bypasses_only_auto_distill() {
    let project_id = ProjectId::from_string("project-2".to_string());
    let (service, outcomes, batches, settings, events) = service_fixture(&project_id).await;
    outcomes
        .upsert(UpsertTaskOutcomeInput {
            outcome: eligible_outcome(&project_id, 1, 0, 20),
        })
        .await
        .expect("seed eligible outcome");
    let mut disabled_automatic = ProjectSkillSettings::default_for_project(project_id.clone());
    disabled_automatic.auto_distill = false;
    settings
        .upsert(disabled_automatic)
        .await
        .expect("disable automatic distillation");

    assert!(service
        .prepare_claim(
            &project_id,
            ProjectSkillDistillationTrigger::Automatic,
            1_800,
        )
        .await
        .expect("automatic gate")
        .is_none());
    assert!(batches
        .list_batched_outcome_ids(&project_id)
        .await
        .expect("list batched outcomes")
        .is_empty());
    let skips = events
        .get_by_type("skill_distillation_skipped")
        .await
        .expect("list skip events");
    assert_eq!(skips.len(), 1);
    assert_eq!(
        skips[0].details["reason"],
        "automatic_distillation_disabled"
    );

    assert!(service
        .prepare_claim(
            &project_id,
            ProjectSkillDistillationTrigger::Explicit,
            1_800,
        )
        .await
        .expect("explicit preparation")
        .is_some());

    let mut disabled = ProjectSkillSettings::default_for_project(project_id.clone());
    disabled.enabled = false;
    settings
        .upsert(disabled)
        .await
        .expect("disable project skills");
    assert!(service
        .prepare_claim(
            &project_id,
            ProjectSkillDistillationTrigger::Explicit,
            1_800,
        )
        .await
        .expect("explicit disabled gate")
        .is_none());
}

#[tokio::test]
async fn explicit_preparation_claims_only_the_selected_outcome() {
    let project_id = ProjectId::from_string("project-explicit".to_string());
    let (service, outcomes, batches, _settings, _events) = service_fixture(&project_id).await;
    let unrelated = eligible_outcome(&project_id, 1, -10, 20);
    let selected = eligible_outcome(&project_id, 2, 0, 20);
    outcomes
        .upsert(UpsertTaskOutcomeInput {
            outcome: unrelated.clone(),
        })
        .await
        .expect("seed unrelated outcome");
    outcomes
        .upsert(UpsertTaskOutcomeInput {
            outcome: selected.clone(),
        })
        .await
        .expect("seed selected outcome");

    let preparation = service
        .prepare_explicit_claims(
            &project_id,
            ProjectSkillDistillationSelection::ExactOutcomes(vec![selected.id.clone()]),
            1_800,
        )
        .await
        .expect("prepare explicit claim");

    assert!(preparation.enabled);
    assert_eq!(preparation.selected_outcomes, 1);
    assert_eq!(preparation.batch_count, 1);
    assert_eq!(preparation.prepared.len(), 1);
    assert_eq!(
        preparation.prepared[0]
            .batch
            .items
            .iter()
            .map(|item| item.outcome_id.as_str())
            .collect::<Vec<_>>(),
        vec![selected.id.as_str()]
    );
    assert_eq!(
        batches
            .list_batched_outcome_ids(&project_id)
            .await
            .expect("list selected batches"),
        vec![selected.id]
    );
}

#[tokio::test]
async fn explicit_eligible_selection_is_bounded_to_ten_outcomes() {
    let project_id = ProjectId::from_string("project-bounded".to_string());
    let (service, outcomes, batches, _settings, _events) = service_fixture(&project_id).await;
    for id in 0..12 {
        outcomes
            .upsert(UpsertTaskOutcomeInput {
                outcome: eligible_outcome(&project_id, id, id as i64, 20),
            })
            .await
            .expect("seed eligible outcome");
    }

    let preparation = service
        .prepare_explicit_claims(
            &project_id,
            ProjectSkillDistillationSelection::EligibleOutcomes {
                source: None,
                limit: 10,
            },
            1_800,
        )
        .await
        .expect("prepare bounded explicit claims");

    assert_eq!(preparation.selected_outcomes, 10);
    assert_eq!(
        batches
            .list_batched_outcome_ids(&project_id)
            .await
            .expect("list bounded outcomes")
            .len(),
        10
    );
}

#[tokio::test]
async fn verification_gap_batch_uses_the_trusted_gap_fingerprint() {
    let project_id = ProjectId::from_string("project-verification".to_string());
    let (service, outcomes, _batches, _settings, _events) = service_fixture(&project_id).await;
    let mut gap = eligible_outcome(&project_id, 1, 0, 20);
    let fingerprint = "b".repeat(64);
    gap.source = TaskOutcomeSource::Verification;
    gap.source_ref_kind = "gap_recurrence".to_string();
    gap.evidence_json = serde_json::json!({
        "fingerprint": fingerprint,
        "description": "The same verification gap recurred.",
    });
    outcomes
        .upsert(UpsertTaskOutcomeInput {
            outcome: gap.clone(),
        })
        .await
        .expect("seed verification gap");

    let preparation = service
        .prepare_explicit_claims(
            &project_id,
            ProjectSkillDistillationSelection::ExactOutcomes(vec![gap.id]),
            1_800,
        )
        .await
        .expect("prepare verification claim");

    assert_eq!(preparation.prepared.len(), 1);
    assert_eq!(preparation.prepared[0].batch.fingerprint, fingerprint);
    assert!(preparation.prepared[0].batch.items[0]
        .digest
        .starts_with(&format!("verification_gap_fingerprint={fingerprint}\n")));
}

#[tokio::test]
async fn recurrence_batch_groups_equivalent_cross_source_evidence() {
    let project_id = ProjectId::from_string("project-recurrence".to_string());
    let (service, outcomes, batches, _settings, _events) = service_fixture(&project_id).await;
    let key = format!("token-set-v1:{}", "c".repeat(64));
    let mut review = eligible_outcome(&project_id, 1, 0, 20);
    review.source = TaskOutcomeSource::Review;
    review.evidence_json = serde_json::json!({
        "summary": "Missing widget",
        "recurrence_key": key,
        "recurrence_session": "session-1",
    });
    let mut merge = eligible_outcome(&project_id, 2, 0, 20);
    merge.source = TaskOutcomeSource::MergeValidation;
    merge.evidence_json = serde_json::json!({
        "summary": "widget missing",
        "recurrence_key": key,
        "recurrence_session": "session-2",
    });
    for outcome in [review.clone(), merge.clone()] {
        outcomes
            .upsert(UpsertTaskOutcomeInput { outcome })
            .await
            .expect("seed recurrence outcome");
    }

    let preparation = service
        .prepare_explicit_claims(
            &project_id,
            ProjectSkillDistillationSelection::ExactOutcomes(vec![
                review.id.clone(),
                merge.id.clone(),
            ]),
            1_800,
        )
        .await
        .expect("prepare recurrence claim");

    assert_eq!(preparation.prepared.len(), 1);
    let batch = batches
        .get_by_outcome_id(&project_id, &review.id)
        .await
        .expect("read recurrence batch")
        .expect("recurrence batch");
    assert_eq!(batch.items.len(), 2);
    assert!(batch
        .items
        .iter()
        .all(|item| item.digest.starts_with(&format!("recurrence_key={key}\n"))));
}
