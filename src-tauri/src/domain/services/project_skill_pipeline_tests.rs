use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::{
    ProjectId, ProjectSkill, ProjectSkillCreatedBy, ProjectSkillEvidenceBatch,
    ProjectSkillEvidenceBatchId, ProjectSkillEvidenceBatchItem, ProjectSkillEvidenceBatchStatus,
    ProjectSkillId, ProjectSkillLifecycleStatus, TaskOutcomeId,
};
use crate::domain::repositories::{
    ProjectSkillEvidenceBatchRepository, ProjectSkillListOptions, ProjectSkillRepository,
    ProjectSkillResolutionOutcome,
};
use crate::domain::services::project_skill_pipeline::{
    ProjectSkillDistillationClaim, ProjectSkillPipelineContext, ProjectSkillPipelineInput,
    ProjectSkillPipelineService, PROJECT_SKILL_BODY_MAX_CHARS,
    PROJECT_SKILL_COMPACT_GUIDANCE_MAX_CHARS, PROJECT_SKILL_PREDICTED_EFFECT_MAX_CHARS,
    PROJECT_SKILL_TITLE_MAX_CHARS,
};
use crate::error::{AppError, AppResult};
use crate::testing::{MemoryProjectSkillEvidenceBatchRepository, MemoryProjectSkillRepository};

fn context(role: &str) -> ProjectSkillPipelineContext {
    ProjectSkillPipelineContext {
        agent_name: match role {
            "memory_capture" => "ralphx-memory-capture",
            "memory_maintainer" => "ralphx-memory-maintainer",
            _ => "ralphx-memory-capture",
        }
        .to_string(),
        pipeline_role: role.to_string(),
        project_id: ProjectId::from_string("project-1".to_string()),
        context_type: "project".to_string(),
        context_id: "project-1".to_string(),
        conversation_id: "conversation-1".to_string(),
        agent_run_id: Some("run-1".to_string()),
        task_id: None,
        distillation_claim: None,
    }
}

fn input(title: &str) -> ProjectSkillPipelineInput {
    ProjectSkillPipelineInput {
        project_id: ProjectId::from_string("project-1".to_string()),
        title: title.to_string(),
        bucket: "execution".to_string(),
        stage: "execution".to_string(),
        scope_paths: vec!["src-tauri/src/domain/**".to_string()],
        compact_guidance: "Route project-skill writes through the owning service.".to_string(),
        body_markdown: "## Procedure\n\n1. Reuse the project-skill resolution seam.".to_string(),
        predicted_effect: "Prevents competing project-skill writers.".to_string(),
    }
}

fn setup() -> (
    Arc<MemoryProjectSkillRepository>,
    ProjectSkillPipelineService,
) {
    let repo = Arc::new(MemoryProjectSkillRepository::new());
    let repository: Arc<dyn ProjectSkillRepository> = repo.clone();
    let service = ProjectSkillPipelineService::new(repository);
    (repo, service)
}

fn evidence_batch(id: &str, outcome_id: &str) -> ProjectSkillEvidenceBatch {
    evidence_batch_with_outcomes(id, &[outcome_id])
}

fn evidence_batch_with_outcomes(id: &str, outcome_ids: &[&str]) -> ProjectSkillEvidenceBatch {
    let now = Utc::now();
    ProjectSkillEvidenceBatch {
        id: ProjectSkillEvidenceBatchId::from_string(id),
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
        items: outcome_ids
            .iter()
            .enumerate()
            .map(|(ordinal, outcome_id)| ProjectSkillEvidenceBatchItem {
                outcome_id: TaskOutcomeId::from_string(*outcome_id),
                ordinal,
                digest: format!("bounded evidence {ordinal}"),
            })
            .collect(),
    }
}

fn distiller_context(batch_id: &str, claim_token: &str) -> ProjectSkillPipelineContext {
    distiller_context_with_outcomes(batch_id, claim_token, &["outcome-1"])
}

fn distiller_context_with_outcomes(
    batch_id: &str,
    claim_token: &str,
    outcome_ids: &[&str],
) -> ProjectSkillPipelineContext {
    let mut runtime = context("skill_distiller");
    runtime.distillation_claim = Some(ProjectSkillDistillationClaim {
        batch_id: ProjectSkillEvidenceBatchId::from_string(batch_id),
        claim_token: claim_token.to_string(),
        fingerprint: "a".repeat(64),
        outcome_ids: outcome_ids
            .iter()
            .map(|outcome_id| TaskOutcomeId::from_string(*outcome_id))
            .collect(),
    });
    runtime
}

struct FailFirstCompletionRepository {
    inner: Arc<MemoryProjectSkillEvidenceBatchRepository>,
    fail_completion: AtomicBool,
}

#[async_trait]
impl ProjectSkillEvidenceBatchRepository for FailFirstCompletionRepository {
    async fn insert_if_absent(
        &self,
        batch: ProjectSkillEvidenceBatch,
    ) -> AppResult<ProjectSkillEvidenceBatch> {
        self.inner.insert_if_absent(batch).await
    }

    async fn list_batched_outcome_ids(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<TaskOutcomeId>> {
        self.inner.list_batched_outcome_ids(project_id).await
    }

    async fn get_by_outcome_id(
        &self,
        project_id: &ProjectId,
        outcome_id: &TaskOutcomeId,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>> {
        self.inner.get_by_outcome_id(project_id, outcome_id).await
    }

    async fn claim_oldest_pending(
        &self,
        project_id: &ProjectId,
        claim_token: &str,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>> {
        self.inner
            .claim_oldest_pending(project_id, claim_token, claimed_at)
            .await
    }

    async fn claim_pending_by_id(
        &self,
        project_id: &ProjectId,
        batch_id: &ProjectSkillEvidenceBatchId,
        claim_token: &str,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>> {
        self.inner
            .claim_pending_by_id(project_id, batch_id, claim_token, claimed_at)
            .await
    }

    async fn release_claim(
        &self,
        batch_id: &ProjectSkillEvidenceBatchId,
        claim_token: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        self.inner
            .release_claim(batch_id, claim_token, updated_at)
            .await
    }

    async fn complete_claim(
        &self,
        batch_id: &ProjectSkillEvidenceBatchId,
        claim_token: &str,
        project_id: &ProjectId,
        project_skill_id: &ProjectSkillId,
        resolution_action: &str,
        completed_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        if self.fail_completion.swap(false, Ordering::SeqCst) {
            return Ok(false);
        }
        self.inner
            .complete_claim(
                batch_id,
                claim_token,
                project_id,
                project_skill_id,
                resolution_action,
                completed_at,
            )
            .await
    }

    async fn requeue_stale_claims(
        &self,
        project_id: &ProjectId,
        stale_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<usize> {
        self.inner
            .requeue_stale_claims(project_id, stale_before, updated_at)
            .await
    }

    async fn get_by_id(
        &self,
        batch_id: &ProjectSkillEvidenceBatchId,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>> {
        self.inner.get_by_id(batch_id).await
    }
}

#[tokio::test]
async fn pipeline_upsert_and_patch_derive_attribution_and_preserve_versions() {
    let (repo, service) = setup();
    let runtime = context("memory_capture");
    let created = service
        .upsert(runtime.clone(), input("Resolution ownership"))
        .await
        .expect("create pipeline skill");

    assert_eq!(created.outcome, ProjectSkillResolutionOutcome::CreateNew);
    assert_eq!(created.skill.created_by, ProjectSkillCreatedBy::Agent);
    assert_eq!(
        created.skill.pipeline_role.as_deref(),
        Some("memory_capture")
    );
    assert_eq!(
        created.skill.provenance_json["source"].as_str(),
        Some("skill_pipeline_mcp")
    );
    assert_eq!(
        created.skill.provenance_json["additional"]["agent_name"].as_str(),
        Some("ralphx-memory-capture")
    );
    assert_eq!(
        created.skill.provenance_json["additional"]["context_id"].as_str(),
        Some("project-1")
    );
    assert_eq!(
        repo.list_versions(&created.skill.id)
            .await
            .expect("versions")
            .len(),
        1
    );

    let duplicate = service
        .upsert(runtime.clone(), input("Resolution ownership"))
        .await
        .expect("duplicate retry");
    assert_eq!(duplicate.outcome, ProjectSkillResolutionOutcome::Duplicate);
    assert!(duplicate.version.is_none());

    let mut revised = input("Resolution ownership");
    revised.body_markdown.push_str("\n2. Verify one writer.");
    let patched = service
        .patch(runtime, created.skill.id.clone(), revised)
        .await
        .expect("patch pipeline skill");
    assert_eq!(
        patched.outcome,
        ProjectSkillResolutionOutcome::PatchExisting
    );
    assert_eq!(patched.skill.id, created.skill.id);
    assert_eq!(
        repo.list_versions(&created.skill.id)
            .await
            .expect("versions")
            .len(),
        2
    );
}

#[tokio::test]
async fn pipeline_validation_rejects_boundaries_templates_and_unsafe_paths_without_writes() {
    let (repo, service) = setup();
    let runtime = context("memory_capture");
    let invalid = [
        {
            let value = input(&"t".repeat(PROJECT_SKILL_TITLE_MAX_CHARS + 1));
            value
        },
        {
            let mut value = input("Compact overflow");
            value.compact_guidance = "g".repeat(PROJECT_SKILL_COMPACT_GUIDANCE_MAX_CHARS + 1);
            value
        },
        {
            let mut value = input("Body overflow");
            value.body_markdown = "b".repeat(PROJECT_SKILL_BODY_MAX_CHARS + 1);
            value
        },
        {
            let mut value = input("Effect overflow");
            value.predicted_effect = "e".repeat(PROJECT_SKILL_PREDICTED_EFFECT_MAX_CHARS + 1);
            value
        },
        input("Draft procedure for legacy output"),
        input("Draft implementation procedure from legacy output"),
        input("Draft procedure from PR #42"),
        {
            let mut value = input("Legacy body marker");
            value.body_markdown = "## Authoring required\n\nRewrite me.".to_string();
            value
        },
        {
            let mut value = input("Unsafe scope");
            value.scope_paths = vec!["../outside".to_string()];
            value
        },
        {
            let mut value = input("Invalid bucket");
            value.bucket = "memory".to_string();
            value
        },
    ];

    for value in invalid {
        assert!(matches!(
            service.upsert(runtime.clone(), value).await,
            Err(AppError::Validation(_))
        ));
    }
    let mut mismatched_authority = context("memory_capture");
    mismatched_authority.agent_name = "ralphx-memory-maintainer".to_string();
    assert!(matches!(
        service
            .upsert(mismatched_authority, input("Invalid authority"))
            .await,
        Err(AppError::Validation(_))
    ));

    let project_id = ProjectId::from_string("project-1".to_string());
    assert!(repo
        .list_by_project(&project_id, ProjectSkillListOptions::default())
        .await
        .expect("rows")
        .is_empty());
}

#[tokio::test]
async fn pipeline_validation_accepts_exact_character_limits() {
    let (_repo, service) = setup();
    let mut exact = input(&"t".repeat(PROJECT_SKILL_TITLE_MAX_CHARS));
    exact.compact_guidance = "g".repeat(PROJECT_SKILL_COMPACT_GUIDANCE_MAX_CHARS);
    exact.body_markdown = "b".repeat(PROJECT_SKILL_BODY_MAX_CHARS);
    exact.predicted_effect = "e".repeat(PROJECT_SKILL_PREDICTED_EFFECT_MAX_CHARS);

    let result = service
        .upsert(context("memory_capture"), exact)
        .await
        .expect("exact limits are valid");
    assert_eq!(result.outcome, ProjectSkillResolutionOutcome::CreateNew);
}

#[tokio::test]
async fn pipeline_retire_is_scoped_guarded_and_idempotent_without_versions() {
    let (repo, service) = setup();
    let runtime = context("memory_maintainer");
    let created = service
        .upsert(runtime.clone(), input("Retirement guard"))
        .await
        .expect("create skill");
    let version_count = repo
        .list_versions(&created.skill.id)
        .await
        .expect("versions")
        .len();

    let retired = service
        .retire(
            runtime.clone(),
            &created.skill.project_id,
            &created.skill.id,
        )
        .await
        .expect("retire skill");
    assert!(retired.changed);
    assert_eq!(retired.skill.status, ProjectSkillLifecycleStatus::Retired);

    let retried = service
        .retire(runtime, &created.skill.project_id, &created.skill.id)
        .await
        .expect("idempotent retire");
    assert!(!retried.changed);
    assert_eq!(retried.skill.updated_at, retired.skill.updated_at);
    assert_eq!(
        repo.list_versions(&created.skill.id)
            .await
            .expect("versions")
            .len(),
        version_count
    );
}

#[tokio::test]
async fn pipeline_patch_of_approved_skill_creates_staged_companion_with_trusted_attribution() {
    let (repo, service) = setup();
    let runtime = context("memory_capture");
    let created = service
        .upsert(runtime.clone(), input("Approved revision"))
        .await
        .expect("create skill");
    repo.update_lifecycle_status(&created.skill.id, ProjectSkillLifecycleStatus::Approved)
        .await
        .expect("approve")
        .expect("skill");

    let mut revised = input("Approved revision");
    revised
        .body_markdown
        .push_str("\n2. Stage a companion revision.");
    let patched = service
        .patch(runtime, created.skill.id.clone(), revised)
        .await
        .expect("stage approved companion");

    assert_eq!(patched.outcome, ProjectSkillResolutionOutcome::CreateNew);
    assert_ne!(patched.skill.id, created.skill.id);
    assert_eq!(
        patched.skill.companion_of_skill_id.as_ref(),
        Some(&created.skill.id)
    );
    assert_eq!(patched.skill.status, ProjectSkillLifecycleStatus::Staged);
    assert_eq!(patched.skill.created_by, ProjectSkillCreatedBy::Agent);
    assert_eq!(
        patched.skill.pipeline_role.as_deref(),
        Some("memory_capture")
    );
    assert_eq!(
        repo.list_versions(&created.skill.id)
            .await
            .expect("approved versions")
            .len(),
        1
    );
    assert_eq!(
        repo.list_versions(&patched.skill.id)
            .await
            .expect("companion versions")
            .len(),
        1
    );
}

#[tokio::test]
async fn pipeline_retire_rejects_excluded_lifecycle_without_mutation() {
    let (repo, service) = setup();
    let runtime = context("memory_maintainer");
    let created = service
        .upsert(runtime.clone(), input("Excluded retirement"))
        .await
        .expect("create skill");
    let rejected = repo
        .update_lifecycle_status(&created.skill.id, ProjectSkillLifecycleStatus::Rejected)
        .await
        .expect("reject")
        .expect("skill");
    let version_count = repo
        .list_versions(&created.skill.id)
        .await
        .expect("versions")
        .len();

    assert!(matches!(
        service
            .retire(runtime, &created.skill.project_id, &created.skill.id)
            .await,
        Err(AppError::Conflict(_))
    ));

    let stored = repo
        .get_by_id(&created.skill.id)
        .await
        .expect("read")
        .expect("skill");
    assert_eq!(stored.status, ProjectSkillLifecycleStatus::Rejected);
    assert_eq!(stored.updated_at, rejected.updated_at);
    assert_eq!(
        repo.list_versions(&created.skill.id)
            .await
            .expect("versions after rejection")
            .len(),
        version_count
    );
}

#[tokio::test]
async fn pipeline_patch_and_retire_reject_cross_project_and_pinned_targets_unchanged() {
    let (repo, service) = setup();
    let runtime = context("memory_maintainer");
    let created = service
        .upsert(runtime.clone(), input("Scoped target"))
        .await
        .expect("create skill");
    repo.update_pinned(&created.skill.id, true)
        .await
        .expect("pin")
        .expect("skill");

    let other_project = ProjectId::from_string("project-2".to_string());
    let mut cross_project_input = input("Cross-project patch");
    cross_project_input.project_id = other_project.clone();
    assert!(matches!(
        service
            .patch(
                runtime.clone(),
                created.skill.id.clone(),
                cross_project_input
            )
            .await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        service
            .retire(runtime, &other_project, &created.skill.id)
            .await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        service
            .retire(
                context("memory_maintainer"),
                &created.skill.project_id,
                &created.skill.id,
            )
            .await,
        Err(AppError::Conflict(_))
    ));

    let stored = repo
        .get_by_id(&ProjectSkillId::from_string(
            created.skill.id.as_str().to_string(),
        ))
        .await
        .expect("read")
        .expect("skill");
    assert!(stored.pinned);
    assert_eq!(stored.status, ProjectSkillLifecycleStatus::Staged);
    assert_eq!(
        repo.list_versions(&stored.id)
            .await
            .expect("versions")
            .len(),
        1
    );
}

#[tokio::test]
async fn skill_distiller_validates_authoritative_claim_and_settles_after_canonical_write() {
    let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
    let batch_repo = Arc::new(MemoryProjectSkillEvidenceBatchRepository::new());
    batch_repo
        .insert_if_absent(evidence_batch_with_outcomes(
            "batch-1",
            &["outcome-1", "outcome-2"],
        ))
        .await
        .expect("insert evidence batch");
    batch_repo
        .claim_oldest_pending(
            &ProjectId::from_string("project-1".to_string()),
            "claim-1",
            Utc::now(),
        )
        .await
        .expect("claim evidence batch")
        .expect("claimed batch");
    let service =
        ProjectSkillPipelineService::with_evidence_batches(skill_repo.clone(), batch_repo.clone());

    let result = service
        .upsert(
            distiller_context_with_outcomes("batch-1", "claim-1", &["outcome-1", "outcome-2"]),
            input("Claim-scoped guidance"),
        )
        .await
        .expect("write and settle claim");
    assert_eq!(result.outcome, ProjectSkillResolutionOutcome::CreateNew);
    assert_eq!(
        result.skill.provenance_json["evidence_batch"]["id"],
        "batch-1"
    );
    assert_eq!(
        result.skill.provenance_json["evidence_batch"]["outcome_ids"],
        serde_json::json!(["outcome-1", "outcome-2"])
    );
    let settled = batch_repo
        .get_by_id(&ProjectSkillEvidenceBatchId::from_string("batch-1"))
        .await
        .expect("read batch")
        .expect("batch exists");
    assert_eq!(
        settled.completed_project_skill_id.as_ref(),
        Some(&result.skill.id)
    );
    assert_eq!(settled.resolution_action.as_deref(), Some("create_new"));
    assert!(settled.completed_at.is_some());
}

#[tokio::test]
async fn verification_claim_uses_trusted_fingerprint_to_create_an_approved_companion() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let fingerprint = "a".repeat(64);
    let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
    let now = Utc::now();
    let approved = skill_repo
        .create(ProjectSkill {
            id: ProjectSkillId::from_string("approved-gap"),
            project_id: project_id.clone(),
            title: "Existing approved verification guidance".to_string(),
            bucket: "verification".to_string(),
            stage: "verification".to_string(),
            status: ProjectSkillLifecycleStatus::Approved,
            pinned: false,
            archived: false,
            scope_paths: Vec::new(),
            compact_guidance: "Check this recurring verification gap.".to_string(),
            body_markdown: "## Procedure\n\nRun the established verification check.".to_string(),
            predicted_effect: Some("Prevents the recurring gap.".to_string()),
            provenance_json: serde_json::json!({
                "additional": { "verification_gap_fingerprint": fingerprint.clone() },
            }),
            companion_of_skill_id: None,
            content_hash: String::new(),
            evidence_hash: String::new(),
            created_by: ProjectSkillCreatedBy::Agent,
            pipeline_role: Some("skill_distiller".to_string()),
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("seed approved verification skill");
    let batch_repo = Arc::new(MemoryProjectSkillEvidenceBatchRepository::new());
    let mut batch = evidence_batch("verification-batch", "gap-outcome");
    batch.bucket = "verification".to_string();
    batch.items[0].digest = format!("verification_gap_fingerprint={fingerprint}\n{{}}");
    batch_repo
        .insert_if_absent(batch)
        .await
        .expect("insert verification evidence batch");
    batch_repo
        .claim_oldest_pending(&project_id, "claim-1", Utc::now())
        .await
        .expect("claim verification batch")
        .expect("claimed verification batch");
    let service =
        ProjectSkillPipelineService::with_evidence_batches(skill_repo, batch_repo.clone());
    let mut authored = input("Agent-authored verification revision");
    authored.bucket = "verification".to_string();
    authored.stage = "verification".to_string();
    let result = service
        .upsert(
            distiller_context_with_outcomes("verification-batch", "claim-1", &["gap-outcome"]),
            authored,
        )
        .await
        .expect("create caller-authored companion");

    assert_eq!(result.outcome, ProjectSkillResolutionOutcome::CreateNew);
    assert_eq!(
        result.skill.companion_of_skill_id.as_ref(),
        Some(&approved.id)
    );
    assert_eq!(result.skill.title, "Agent-authored verification revision");
    assert_eq!(
        result.skill.provenance_json["additional"]["verification_gap_fingerprint"],
        fingerprint
    );
    assert!(batch_repo
        .get_by_id(&ProjectSkillEvidenceBatchId::from_string(
            "verification-batch"
        ))
        .await
        .expect("read verification batch")
        .expect("verification batch exists")
        .completed_at
        .is_some());
}

#[tokio::test]
async fn non_gap_verification_claim_does_not_reuse_gap_identity() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let fingerprint = "a".repeat(64);
    let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
    let now = Utc::now();
    let approved = skill_repo
        .create(ProjectSkill {
            id: ProjectSkillId::from_string("approved-gap"),
            project_id: project_id.clone(),
            title: "Existing approved verification guidance".to_string(),
            bucket: "verification".to_string(),
            stage: "verification".to_string(),
            status: ProjectSkillLifecycleStatus::Approved,
            pinned: false,
            archived: false,
            scope_paths: Vec::new(),
            compact_guidance: "Check this recurring verification gap.".to_string(),
            body_markdown: "## Procedure\n\nRun the established verification check.".to_string(),
            predicted_effect: Some("Prevents the recurring gap.".to_string()),
            provenance_json: serde_json::json!({
                "additional": { "verification_gap_fingerprint": fingerprint },
            }),
            companion_of_skill_id: None,
            content_hash: String::new(),
            evidence_hash: String::new(),
            created_by: ProjectSkillCreatedBy::Agent,
            pipeline_role: Some("skill_distiller".to_string()),
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("seed approved verification skill");
    let batch_repo = Arc::new(MemoryProjectSkillEvidenceBatchRepository::new());
    let mut batch = evidence_batch("verification-batch", "verification-outcome");
    batch.bucket = "verification".to_string();
    batch.items[0].digest = serde_json::json!({
        "source": "verification",
        "source_ref_kind": "run_result",
        "evidence": {},
    })
    .to_string();
    batch_repo.insert_if_absent(batch).await.unwrap();
    batch_repo
        .claim_oldest_pending(&project_id, "claim-1", Utc::now())
        .await
        .unwrap();
    let service =
        ProjectSkillPipelineService::with_evidence_batches(skill_repo, batch_repo.clone());
    let mut authored = input("Independent verification guidance");
    authored.bucket = "verification".to_string();
    authored.stage = "verification".to_string();
    let result = service
        .upsert(
            distiller_context_with_outcomes(
                "verification-batch",
                "claim-1",
                &["verification-outcome"],
            ),
            authored,
        )
        .await
        .expect("create independent verification skill");

    assert_ne!(result.skill.id, approved.id);
    assert_eq!(result.skill.companion_of_skill_id, None);
    assert!(result.skill.provenance_json["additional"]
        .get("verification_gap_fingerprint")
        .is_none());
}

#[tokio::test]
async fn skill_distiller_rejects_mismatched_persisted_claim_before_any_write() {
    let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
    let batch_repo = Arc::new(MemoryProjectSkillEvidenceBatchRepository::new());
    batch_repo
        .insert_if_absent(evidence_batch("batch-1", "outcome-1"))
        .await
        .expect("insert evidence batch");
    batch_repo
        .claim_oldest_pending(
            &ProjectId::from_string("project-1".to_string()),
            "authoritative-token",
            Utc::now(),
        )
        .await
        .expect("claim evidence batch")
        .expect("claimed batch");
    let service =
        ProjectSkillPipelineService::with_evidence_batches(skill_repo.clone(), batch_repo);

    assert!(matches!(
        service
            .upsert(
                distiller_context("batch-1", "spoofed-token"),
                input("Must not be written"),
            )
            .await,
        Err(AppError::Conflict(_))
    ));
    assert!(skill_repo
        .list_by_project(
            &ProjectId::from_string("project-1".to_string()),
            ProjectSkillListOptions::default(),
        )
        .await
        .expect("list project skills")
        .is_empty());
}

#[tokio::test]
async fn skill_distiller_patch_persists_provenance_before_settlement() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
    let ordinary_service = ProjectSkillPipelineService::new(skill_repo.clone());
    let created = ordinary_service
        .upsert(context("memory_capture"), input("Patchable guidance"))
        .await
        .expect("create skill");
    let batch_repo = Arc::new(MemoryProjectSkillEvidenceBatchRepository::new());
    batch_repo
        .insert_if_absent(evidence_batch("batch-1", "outcome-1"))
        .await
        .expect("insert evidence batch");
    batch_repo
        .claim_oldest_pending(&project_id, "claim-1", Utc::now())
        .await
        .expect("claim evidence batch")
        .expect("claimed batch");
    let distiller_service =
        ProjectSkillPipelineService::with_evidence_batches(skill_repo, batch_repo.clone());
    let mut revised = input("Patchable guidance");
    revised
        .body_markdown
        .push_str("\n2. Preserve batch provenance.");

    let patched = distiller_service
        .patch(
            distiller_context("batch-1", "claim-1"),
            created.skill.id.clone(),
            revised,
        )
        .await
        .expect("patch and settle");
    assert_eq!(
        patched.outcome,
        ProjectSkillResolutionOutcome::PatchExisting
    );
    assert_eq!(
        patched.skill.provenance_json["evidence_batch"]["id"],
        "batch-1"
    );
    let settled = batch_repo
        .get_by_id(&ProjectSkillEvidenceBatchId::from_string("batch-1"))
        .await
        .expect("read batch")
        .expect("batch exists");
    assert_eq!(settled.resolution_action.as_deref(), Some("patch_existing"));
    assert_eq!(
        settled.completed_project_skill_id.as_ref(),
        Some(&patched.skill.id)
    );
}

#[tokio::test]
async fn marker_failure_reports_false_success_and_duplicate_retry_converges() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
    let batch_repo = Arc::new(MemoryProjectSkillEvidenceBatchRepository::new());
    batch_repo
        .insert_if_absent(evidence_batch("batch-1", "outcome-1"))
        .await
        .expect("insert evidence batch");
    batch_repo
        .claim_oldest_pending(&project_id, "claim-1", Utc::now())
        .await
        .expect("claim evidence batch")
        .expect("claimed batch");
    let failing_repo = Arc::new(FailFirstCompletionRepository {
        inner: batch_repo.clone(),
        fail_completion: AtomicBool::new(true),
    });
    let failing_service =
        ProjectSkillPipelineService::with_evidence_batches(skill_repo.clone(), failing_repo);

    assert!(matches!(
        failing_service
            .upsert(
                distiller_context("batch-1", "claim-1"),
                input("Retry-safe guidance"),
            )
            .await,
        Err(AppError::Conflict(_))
    ));
    let unmarked = batch_repo
        .get_by_id(&ProjectSkillEvidenceBatchId::from_string("batch-1"))
        .await
        .expect("read unmarked batch")
        .expect("batch exists");
    assert!(unmarked.completed_at.is_none());
    assert_eq!(
        skill_repo
            .list_by_project(&project_id, ProjectSkillListOptions::default())
            .await
            .expect("list project skills")
            .len(),
        1
    );

    let retry_service =
        ProjectSkillPipelineService::with_evidence_batches(skill_repo.clone(), batch_repo.clone());
    let retried = retry_service
        .upsert(
            distiller_context("batch-1", "claim-1"),
            input("Retry-safe guidance"),
        )
        .await
        .expect("duplicate retry settles");
    assert_eq!(retried.outcome, ProjectSkillResolutionOutcome::Duplicate);
    assert_eq!(
        skill_repo
            .list_versions(&retried.skill.id)
            .await
            .expect("list versions")
            .len(),
        1
    );
    let settled = batch_repo
        .get_by_id(&ProjectSkillEvidenceBatchId::from_string("batch-1"))
        .await
        .expect("read settled batch")
        .expect("batch exists");
    assert_eq!(settled.resolution_action.as_deref(), Some("duplicate"));
    assert!(settled.completed_at.is_some());
}

#[tokio::test]
async fn skill_distiller_retire_does_not_falsely_settle_authoring_claim() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
    let ordinary_service = ProjectSkillPipelineService::new(skill_repo.clone());
    let created = ordinary_service
        .upsert(context("memory_capture"), input("Retirable guidance"))
        .await
        .expect("create skill");
    let batch_repo = Arc::new(MemoryProjectSkillEvidenceBatchRepository::new());
    batch_repo
        .insert_if_absent(evidence_batch("batch-1", "outcome-1"))
        .await
        .expect("insert evidence batch");
    batch_repo
        .claim_oldest_pending(&project_id, "claim-1", Utc::now())
        .await
        .expect("claim evidence batch")
        .expect("claimed batch");
    let distiller_service =
        ProjectSkillPipelineService::with_evidence_batches(skill_repo, batch_repo.clone());

    let retired = distiller_service
        .retire(
            distiller_context("batch-1", "claim-1"),
            &project_id,
            &created.skill.id,
        )
        .await
        .expect("retire and settle");
    assert!(retired.changed);
    assert_eq!(retired.skill.status, ProjectSkillLifecycleStatus::Retired);
    let uncompleted = batch_repo
        .get_by_id(&ProjectSkillEvidenceBatchId::from_string("batch-1"))
        .await
        .expect("read batch")
        .expect("batch exists");
    assert!(uncompleted.resolution_action.is_none());
    assert!(uncompleted.completed_project_skill_id.is_none());
    assert!(uncompleted.completed_at.is_none());
}
