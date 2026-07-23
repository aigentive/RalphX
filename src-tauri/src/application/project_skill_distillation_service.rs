use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};

use crate::domain::entities::{
    MemoryActorType, MemoryEvent, ProjectId, ProjectSkillEvidenceBatch,
    ProjectSkillEvidenceBatchId, ProjectSkillEvidenceBatchItem, ProjectSkillEvidenceBatchStatus,
    ProjectSkillLifecycleStatus, TaskOutcome, TaskOutcomeId, TaskOutcomeStatus,
    PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS, PROJECT_SKILL_EVIDENCE_DIGEST_MAX_CHARS,
};
use crate::domain::repositories::{
    MemoryEventRepository, ProjectSkillEvidenceBatchRepository, ProjectSkillListOptions,
    ProjectSkillRepository, ProjectSkillSettingsRepository, TaskOutcomeListOptions,
    TaskOutcomeRepository,
};
use crate::domain::services::learned_skill_substrate::bucket_for_outcome_source;
use crate::error::AppResult;

pub const SKILL_DISTILLER_PROFILE: &str = "skill_distiller";
pub const SKILL_DISTILLER_PROMPT_INDEX_LIMIT: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSkillDistillationTrigger {
    Automatic,
    Explicit,
}

#[derive(Debug, Clone)]
pub struct PreparedProjectSkillDistillation {
    pub batch: ProjectSkillEvidenceBatch,
    pub claim_token: String,
    pub prompt: String,
}

pub struct ProjectSkillDistillationService {
    outcome_repo: Arc<dyn TaskOutcomeRepository>,
    batch_repo: Arc<dyn ProjectSkillEvidenceBatchRepository>,
    settings_repo: Arc<dyn ProjectSkillSettingsRepository>,
    skill_repo: Arc<dyn ProjectSkillRepository>,
    memory_event_repo: Arc<dyn MemoryEventRepository>,
}

impl ProjectSkillDistillationService {
    pub fn new(
        outcome_repo: Arc<dyn TaskOutcomeRepository>,
        batch_repo: Arc<dyn ProjectSkillEvidenceBatchRepository>,
        settings_repo: Arc<dyn ProjectSkillSettingsRepository>,
        skill_repo: Arc<dyn ProjectSkillRepository>,
        memory_event_repo: Arc<dyn MemoryEventRepository>,
    ) -> Self {
        Self {
            outcome_repo,
            batch_repo,
            settings_repo,
            skill_repo,
            memory_event_repo,
        }
    }

    pub async fn prepare_claim(
        &self,
        project_id: &ProjectId,
        trigger: ProjectSkillDistillationTrigger,
        stale_after_secs: u64,
    ) -> AppResult<Option<PreparedProjectSkillDistillation>> {
        let settings = self
            .settings_repo
            .get_for_project(project_id)
            .await?
            .unwrap_or_else(|| {
                crate::domain::entities::ProjectSkillSettings::default_for_project(
                    project_id.clone(),
                )
            });
        if !settings.enabled {
            self.record_skip(project_id, "project_skills_disabled")
                .await?;
            return Ok(None);
        }
        if trigger == ProjectSkillDistillationTrigger::Automatic && !settings.auto_distill {
            self.record_skip(project_id, "automatic_distillation_disabled")
                .await?;
            return Ok(None);
        }

        let now = Utc::now();
        let stale_seconds = i64::try_from(stale_after_secs).unwrap_or(i64::MAX);
        let stale_before = now
            .checked_sub_signed(Duration::seconds(stale_seconds))
            .unwrap_or(DateTime::<Utc>::MIN_UTC);
        self.batch_repo
            .requeue_stale_claims(project_id, stale_before, now)
            .await?;
        self.enqueue_unbatched_outcomes(project_id).await?;

        let claim_token = uuid::Uuid::new_v4().to_string();
        let Some(batch) = self
            .batch_repo
            .claim_oldest_pending(project_id, &claim_token, now)
            .await?
        else {
            return Ok(None);
        };
        let prompt = match self.render_prompt(&batch).await {
            Ok(prompt) => prompt,
            Err(error) => {
                if let Err(release_error) = self
                    .batch_repo
                    .release_claim(&batch.id, &claim_token, Utc::now())
                    .await
                {
                    tracing::warn!(
                        batch_id = batch.id.as_str(),
                        error = %release_error,
                        "Failed to release skill distillation claim after prompt rendering failed"
                    );
                }
                return Err(error);
            }
        };
        Ok(Some(PreparedProjectSkillDistillation {
            batch,
            claim_token,
            prompt,
        }))
    }

    async fn enqueue_unbatched_outcomes(&self, project_id: &ProjectId) -> AppResult<()> {
        let mut outcomes = self
            .outcome_repo
            .list_by_project(
                project_id,
                TaskOutcomeListOptions {
                    source: None,
                    status: Some(TaskOutcomeStatus::Eligible),
                },
            )
            .await?;
        let batched = self
            .batch_repo
            .list_batched_outcome_ids(project_id)
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        outcomes.retain(|outcome| !batched.contains(&outcome.id));
        outcomes.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        });

        let mut by_bucket = BTreeMap::<String, Vec<TaskOutcome>>::new();
        for outcome in outcomes {
            by_bucket
                .entry(bucket_for_outcome_source(&outcome.source).to_string())
                .or_default()
                .push(outcome);
        }

        for (bucket, outcomes) in by_bucket {
            for chunk in outcomes.chunks(PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS) {
                self.batch_repo
                    .insert_if_absent(build_batch(project_id, &bucket, chunk))
                    .await?;
            }
        }
        Ok(())
    }

    async fn render_prompt(&self, batch: &ProjectSkillEvidenceBatch) -> AppResult<String> {
        let mut skills = self
            .skill_repo
            .list_by_project(
                &batch.project_id,
                ProjectSkillListOptions {
                    bucket: Some(batch.bucket.clone()),
                    include_archived: false,
                    ..Default::default()
                },
            )
            .await?;
        skills.retain(|skill| {
            matches!(
                skill.status,
                ProjectSkillLifecycleStatus::Staged | ProjectSkillLifecycleStatus::Approved
            ) && !skill.archived
        });
        skills.sort_by(|left, right| {
            left.title
                .cmp(&right.title)
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        });
        skills.truncate(SKILL_DISTILLER_PROMPT_INDEX_LIMIT);

        let evidence = batch
            .items
            .iter()
            .map(|item| {
                format!(
                    "- outcome_id=`{}`\n  digest: {}",
                    item.outcome_id.as_str(),
                    item.digest
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let index = if skills.is_empty() {
            "- none".to_string()
        } else {
            skills
                .iter()
                .map(|skill| format!("- `{}` — `{}`", skill.title, skill.content_hash))
                .collect::<Vec<_>>()
                .join("\n")
        };

        Ok(format!(
            "Author reusable project guidance from the bounded evidence below.\n\
             Bucket and stage: `{bucket}`.\n\
             Evidence batch fingerprint: `{fingerprint}`.\n\n\
             Evidence digests (maximum 8; each maximum 1,200 characters):\n{evidence}\n\n\
             Active/staged same-bucket skill index (title — content hash; maximum {index_limit}):\n\
             {index}\n\n\
             Authoring contract:\n\
             - Produce a reusable project procedure centered on current project behavior.\n\
             - Consolidate related evidence into the smallest useful skill.\n\
             - Use only upsert_project_skill, patch_project_skill, or retire_project_skill.\n\
             - Respect the supplied project, bucket, project-skill field caps, and staged-skill limits.\n\
             - Runtime attribution, evidence identity, and provenance are backend-owned.\n\
             - If no durable reusable guidance is justified, make no authoring write.",
            bucket = batch.bucket,
            fingerprint = batch.fingerprint,
            index_limit = SKILL_DISTILLER_PROMPT_INDEX_LIMIT,
        ))
    }

    async fn record_skip(&self, project_id: &ProjectId, reason: &str) -> AppResult<()> {
        self.memory_event_repo
            .create(MemoryEvent::new(
                project_id.clone(),
                "skill_distillation_skipped",
                MemoryActorType::System,
                serde_json::json!({ "reason": reason }),
            ))
            .await?;
        Ok(())
    }
}

fn build_batch(
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
    let fingerprint = format!("{:x}", Sha256::digest(canonical.to_string().as_bytes()));
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
    canonical
        .chars()
        .take(PROJECT_SKILL_EVIDENCE_DIGEST_MAX_CHARS)
        .collect()
}

pub fn claim_outcome_ids(batch: &ProjectSkillEvidenceBatch) -> Vec<TaskOutcomeId> {
    batch
        .items
        .iter()
        .map(|item| item.outcome_id.clone())
        .collect()
}
