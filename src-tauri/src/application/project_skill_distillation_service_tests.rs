use std::sync::Arc;

use chrono::{Duration, Utc};

use super::project_skill_distillation_service::{
    ProjectSkillDistillationService, ProjectSkillDistillationTrigger,
};
use crate::domain::entities::{
    ProjectId, ProjectSkillSettings, TaskOutcome, TaskOutcomeId, TaskOutcomeStatus,
    PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS, PROJECT_SKILL_EVIDENCE_DIGEST_MAX_CHARS,
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
        source: "task_pipeline".to_string(),
        source_ref_kind: "task".to_string(),
        source_ref_id: format!("task-{id:02}"),
        task_id: Some(format!("task-{id:02}")),
        conversation_id: Some("conversation-1".to_string()),
        agent_run_id: Some(format!("run-{id:02}")),
        pull_request_id: None,
        proposal_id: None,
        verification_id: None,
        review_id: None,
        outcome_class: Some("completed".to_string()),
        status: TaskOutcomeStatus::Eligible,
        evidence_json: serde_json::json!({ "summary": "é".repeat(evidence_chars) }),
        provider_harness: Some("codex".to_string()),
        provider_session_id: Some(format!("session-{id:02}")),
        created_at,
        updated_at: created_at,
    }
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
