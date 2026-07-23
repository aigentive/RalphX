use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use crate::domain::entities::{
    MemoryActorType, MemoryEvent, ProjectId, ProjectSkillEvidenceBatch,
    ProjectSkillEvidenceBatchStatus, ProjectSkillLifecycleStatus, TaskOutcome, TaskOutcomeId,
    TaskOutcomeSource, TaskOutcomeStatus, PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS,
};
use crate::domain::repositories::{
    MemoryEventRepository, ProjectSkillEvidenceBatchRepository, ProjectSkillListOptions,
    ProjectSkillRepository, ProjectSkillSettingsRepository, TaskOutcomeListOptions,
    TaskOutcomeRepository,
};
use crate::error::{AppError, AppResult};
use chrono::{DateTime, Duration, Utc};

use super::project_skill_distillation_batching::{
    bucket_for_outcome_source, build_batch, recurrence_key, verification_gap_fingerprint,
};

pub const SKILL_DISTILLER_PROFILE: &str = "skill_distiller";
pub const SKILL_DISTILLER_PROMPT_INDEX_LIMIT: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSkillDistillationTrigger {
    Automatic,
    Explicit,
}

#[derive(Debug, Clone)]
pub enum ProjectSkillDistillationSelection {
    EligibleOutcomes {
        source: Option<TaskOutcomeSource>,
        limit: usize,
    },
    ExactOutcomes(Vec<TaskOutcomeId>),
}

#[derive(Debug, Clone)]
pub struct PreparedExplicitProjectSkillDistillation {
    pub enabled: bool,
    pub selected_outcomes: usize,
    pub batch_count: usize,
    pub prepared: Vec<PreparedProjectSkillDistillation>,
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

    pub async fn prepare_explicit_claims(
        &self,
        project_id: &ProjectId,
        selection: ProjectSkillDistillationSelection,
        stale_after_secs: u64,
    ) -> AppResult<PreparedExplicitProjectSkillDistillation> {
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
            return Ok(PreparedExplicitProjectSkillDistillation {
                enabled: false,
                selected_outcomes: 0,
                batch_count: 0,
                prepared: Vec::new(),
            });
        }

        let now = Utc::now();
        let stale_seconds = i64::try_from(stale_after_secs).unwrap_or(i64::MAX);
        let stale_before = now
            .checked_sub_signed(Duration::seconds(stale_seconds))
            .unwrap_or(DateTime::<Utc>::MIN_UTC);
        self.batch_repo
            .requeue_stale_claims(project_id, stale_before, now)
            .await?;

        let outcomes = self
            .selected_eligible_outcomes(project_id, selection)
            .await?;
        self.enqueue_outcomes(project_id, &outcomes).await?;
        let selected_ids = outcomes
            .iter()
            .map(|outcome| outcome.id.clone())
            .collect::<HashSet<_>>();
        let mut batches = BTreeMap::new();
        for outcome in &outcomes {
            if let Some(batch) = self
                .batch_repo
                .get_by_outcome_id(project_id, &outcome.id)
                .await?
            {
                batches
                    .entry(batch.id.as_str().to_string())
                    .or_insert(batch);
            }
        }

        let batch_count = batches.len();
        let mut prepared = Vec::new();
        for batch in batches.into_values() {
            if batch.status != ProjectSkillEvidenceBatchStatus::Pending
                || !batch
                    .items
                    .iter()
                    .all(|item| selected_ids.contains(&item.outcome_id))
            {
                continue;
            }
            let claim_token = uuid::Uuid::new_v4().to_string();
            let Some(claimed) = self
                .batch_repo
                .claim_pending_by_id(project_id, &batch.id, &claim_token, Utc::now())
                .await?
            else {
                continue;
            };
            let prompt = match self.render_prompt(&claimed).await {
                Ok(prompt) => prompt,
                Err(error) => {
                    if let Err(release_error) = self
                        .batch_repo
                        .release_claim(&claimed.id, &claim_token, Utc::now())
                        .await
                    {
                        tracing::warn!(
                            batch_id = claimed.id.as_str(),
                            error = %release_error,
                            "Failed to release explicit skill distillation claim after prompt rendering failed"
                        );
                    }
                    return Err(error);
                }
            };
            prepared.push(PreparedProjectSkillDistillation {
                batch: claimed,
                claim_token,
                prompt,
            });
        }

        Ok(PreparedExplicitProjectSkillDistillation {
            enabled: true,
            selected_outcomes: outcomes.len(),
            batch_count,
            prepared,
        })
    }

    async fn selected_eligible_outcomes(
        &self,
        project_id: &ProjectId,
        selection: ProjectSkillDistillationSelection,
    ) -> AppResult<Vec<TaskOutcome>> {
        match selection {
            ProjectSkillDistillationSelection::EligibleOutcomes { source, limit } => Ok(self
                .outcome_repo
                .list_by_project(
                    project_id,
                    TaskOutcomeListOptions {
                        source,
                        status: Some(TaskOutcomeStatus::Eligible),
                    },
                )
                .await?
                .into_iter()
                .take(limit.clamp(1, 10))
                .collect()),
            ProjectSkillDistillationSelection::ExactOutcomes(outcome_ids) => {
                let mut selected = Vec::new();
                let mut seen = HashSet::new();
                for outcome_id in outcome_ids {
                    if !seen.insert(outcome_id.clone()) {
                        continue;
                    }
                    let outcome =
                        self.outcome_repo
                            .get_by_id(&outcome_id)
                            .await?
                            .ok_or_else(|| {
                                AppError::NotFound(format!(
                                    "task outcome {} was not found",
                                    outcome_id.as_str()
                                ))
                            })?;
                    if outcome.project_id != *project_id {
                        return Err(AppError::Validation(
                            "selected task outcome belongs to a different project".to_string(),
                        ));
                    }
                    if outcome.status == TaskOutcomeStatus::Eligible {
                        selected.push(outcome);
                    }
                }
                Ok(selected)
            }
        }
    }

    async fn enqueue_unbatched_outcomes(&self, project_id: &ProjectId) -> AppResult<()> {
        let outcomes = self
            .outcome_repo
            .list_by_project(
                project_id,
                TaskOutcomeListOptions {
                    source: None,
                    status: Some(TaskOutcomeStatus::Eligible),
                },
            )
            .await?;
        self.enqueue_outcomes(project_id, &outcomes).await
    }

    async fn enqueue_outcomes(
        &self,
        project_id: &ProjectId,
        outcomes: &[TaskOutcome],
    ) -> AppResult<()> {
        let batched = self
            .batch_repo
            .list_batched_outcome_ids(project_id)
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        let mut outcomes = outcomes
            .iter()
            .filter(|outcome| !batched.contains(&outcome.id))
            .cloned()
            .collect::<Vec<_>>();
        outcomes.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        });

        let mut by_recurrence = BTreeMap::<String, Vec<TaskOutcome>>::new();
        let mut by_bucket = BTreeMap::<String, Vec<TaskOutcome>>::new();
        for outcome in outcomes {
            if let Some(key) = recurrence_key(&outcome).map(str::to_string) {
                by_recurrence.entry(key).or_default().push(outcome);
                continue;
            }
            if verification_gap_fingerprint(&outcome).is_some() {
                self.batch_repo
                    .insert_if_absent(build_batch(
                        project_id,
                        bucket_for_outcome_source(outcome.source),
                        std::slice::from_ref(&outcome),
                    ))
                    .await?;
                continue;
            }
            by_bucket
                .entry(bucket_for_outcome_source(outcome.source).to_string())
                .or_default()
                .push(outcome);
        }

        for outcomes in by_recurrence.into_values() {
            for chunk in outcomes.chunks(PROJECT_SKILL_EVIDENCE_BATCH_MAX_ITEMS) {
                let bucket = chunk
                    .first()
                    .map(|outcome| bucket_for_outcome_source(outcome.source))
                    .unwrap_or("execution");
                self.batch_repo
                    .insert_if_absent(build_batch(project_id, bucket, chunk))
                    .await?;
            }
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

pub fn claim_outcome_ids(batch: &ProjectSkillEvidenceBatch) -> Vec<TaskOutcomeId> {
    batch
        .items
        .iter()
        .map(|item| item.outcome_id.clone())
        .collect()
}
