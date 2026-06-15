use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    MemoryEntry, MemoryEntryId, MemoryStatus, ProjectSkill, ProjectSkillId,
    ProjectSkillLifecycleStatus, SkillUsageEvent, SkillUsageEventId, TaskOutcome, TaskOutcomeId,
    TaskOutcomeStatus,
};
use crate::domain::repositories::{
    MemoryEntryRepository, ProjectSkillListOptions, ProjectSkillRepository,
    SkillUsageEventRepository, SkillUsageListOptions, TaskOutcomeListOptions,
    TaskOutcomeRepository, UpsertTaskOutcomeInput,
};
use crate::domain::services::learned_skill_adapters::LearnedSkillConstraintCitation;
use crate::error::{AppError, AppResult};

const PROJECT_SKILL_BUCKET_VALUES: &[&str] =
    &["planning", "verification", "review", "execution", "merge"];
const PROJECT_SKILL_STAGE_VALUES: &[&str] = PROJECT_SKILL_BUCKET_VALUES;

pub struct OutcomeLedgerService {
    repo: Arc<dyn TaskOutcomeRepository>,
}

impl OutcomeLedgerService {
    pub fn new(repo: Arc<dyn TaskOutcomeRepository>) -> Self {
        Self { repo }
    }

    pub async fn record_outcome(&self, outcome: TaskOutcome) -> AppResult<TaskOutcome> {
        validate_non_empty("outcome source", &outcome.source)?;
        validate_non_empty("outcome source_ref_kind", &outcome.source_ref_kind)?;
        validate_non_empty("outcome source_ref_id", &outcome.source_ref_id)?;
        self.repo.upsert(UpsertTaskOutcomeInput { outcome }).await
    }

    pub async fn list_project_outcomes(
        &self,
        project_id: &ProjectId,
        options: TaskOutcomeListOptions,
    ) -> AppResult<Vec<TaskOutcome>> {
        self.repo.list_by_project(project_id, options).await
    }
}

pub struct ProjectSkillService {
    repo: Arc<dyn ProjectSkillRepository>,
}

pub struct ProjectSkillDistillerService {
    outcome_repo: Arc<dyn TaskOutcomeRepository>,
    skill_service: ProjectSkillService,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSkillDistillationOrigin {
    ManualCurator,
    VerificationObserver,
    PlanModeObserver,
    MemoryPipelineRole,
    DeterministicService,
}

impl ProjectSkillDistillationOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManualCurator => "manual_curator",
            Self::VerificationObserver => "verification_observer",
            Self::PlanModeObserver => "plan_mode_observer",
            Self::MemoryPipelineRole => "memory_pipeline_role",
            Self::DeterministicService => "deterministic_service",
        }
    }

    pub fn pipeline_role(self) -> Option<&'static str> {
        match self {
            Self::MemoryPipelineRole => Some("memory_capture.skill_distiller"),
            _ => None,
        }
    }
}

pub struct StageProjectSkillFromOutcomeInput {
    pub outcome: TaskOutcome,
    pub title: String,
    pub bucket: String,
    pub stage: String,
    pub scope_paths: Vec<String>,
    pub compact_guidance: String,
    pub body_markdown: String,
    pub predicted_effect: String,
    pub additional_provenance: Value,
}

pub struct UpdateProjectSkillContentInput {
    pub project_skill_id: ProjectSkillId,
    pub title: String,
    pub bucket: String,
    pub stage: String,
    pub scope_paths: Vec<String>,
    pub compact_guidance: String,
    pub body_markdown: String,
    pub predicted_effect: String,
    pub source_sync_enabled: Option<bool>,
}

pub struct DistillEligibleOutcomesInput {
    pub project_id: ProjectId,
    pub source: Option<String>,
    pub limit: usize,
    pub origin: ProjectSkillDistillationOrigin,
}

#[derive(Debug, Clone)]
pub struct DistillEligibleOutcomesResult {
    pub staged_skills: Vec<ProjectSkill>,
    pub skipped_existing: usize,
}

impl ProjectSkillService {
    pub fn new(repo: Arc<dyn ProjectSkillRepository>) -> Self {
        Self { repo }
    }

    pub async fn stage_skill(&self, skill: ProjectSkill) -> AppResult<ProjectSkill> {
        validate_project_skill(&skill)?;
        if skill
            .predicted_effect
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return Err(AppError::Validation(
                "staged project skills require predicted_effect".to_string(),
            ));
        }
        self.repo.create(skill).await
    }

    pub async fn stage_skill_from_outcome(
        &self,
        input: StageProjectSkillFromOutcomeInput,
    ) -> AppResult<ProjectSkill> {
        if input.outcome.status != TaskOutcomeStatus::Eligible {
            return Err(AppError::Validation(
                "project skill distillation requires an eligible task outcome".to_string(),
            ));
        }
        validate_non_empty("predicted_effect", &input.predicted_effect)?;

        let now = Utc::now();
        let skill = ProjectSkill {
            id: ProjectSkillId::new(),
            project_id: input.outcome.project_id.clone(),
            title: input.title,
            bucket: input.bucket,
            stage: input.stage,
            status: ProjectSkillLifecycleStatus::Staged,
            pinned: false,
            archived: false,
            scope_paths: input.scope_paths,
            compact_guidance: input.compact_guidance,
            body_markdown: input.body_markdown,
            predicted_effect: Some(input.predicted_effect),
            provenance_json: serde_json::json!({
                "source": "task_outcome",
                "outcome_id": input.outcome.id.as_str(),
                "outcome_source": input.outcome.source,
                "outcome_source_ref_kind": input.outcome.source_ref_kind,
                "outcome_source_ref_id": input.outcome.source_ref_id,
                "outcome_class": input.outcome.outcome_class,
                "task_id": input.outcome.task_id,
                "agent_run_id": input.outcome.agent_run_id,
                "review_id": input.outcome.review_id,
                "additional": input.additional_provenance,
            }),
            companion_of_skill_id: None,
            created_at: now,
            updated_at: now,
        };

        self.stage_skill(skill).await
    }

    pub async fn get_skill(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
        self.repo.get_by_id(id).await
    }

    pub async fn list_project_skills(
        &self,
        project_id: &ProjectId,
        options: ProjectSkillListOptions,
    ) -> AppResult<Vec<ProjectSkill>> {
        self.repo.list_by_project(project_id, options).await
    }

    pub async fn update_skill_content(
        &self,
        input: UpdateProjectSkillContentInput,
    ) -> AppResult<Option<ProjectSkill>> {
        validate_non_empty("project skill title", &input.title)?;
        validate_project_skill_bucket(&input.bucket)?;
        validate_project_skill_stage(&input.stage)?;
        validate_non_empty("project skill compact_guidance", &input.compact_guidance)?;
        validate_non_empty("project skill body_markdown", &input.body_markdown)?;
        validate_non_empty("predicted_effect", &input.predicted_effect)?;

        let Some(mut skill) = self.repo.get_by_id(&input.project_skill_id).await? else {
            return Ok(None);
        };
        if skill.archived
            || matches!(
                skill.status,
                ProjectSkillLifecycleStatus::Archived | ProjectSkillLifecycleStatus::Retired
            )
        {
            return Err(AppError::Validation(
                "archived or retired project skills cannot be edited".to_string(),
            ));
        }
        skill.title = input.title;
        skill.bucket = input.bucket;
        skill.stage = input.stage;
        skill.scope_paths = input.scope_paths;
        skill.compact_guidance = input.compact_guidance;
        skill.body_markdown = input.body_markdown;
        skill.predicted_effect = Some(input.predicted_effect);
        if let Some(source_sync_enabled) = input.source_sync_enabled {
            set_project_skill_source_sync_enabled(&mut skill.provenance_json, source_sync_enabled);
        }
        self.repo.update_content(skill).await
    }

    pub async fn prompt_selected_citations(
        &self,
        project_id: &ProjectId,
        prompt: &str,
    ) -> AppResult<Vec<LearnedSkillConstraintCitation>> {
        Ok(self
            .prompt_selected_skills(project_id, prompt)
            .await?
            .into_iter()
            .map(project_skill_to_constraint_citation)
            .collect())
    }

    pub async fn prompt_selected_skills(
        &self,
        project_id: &ProjectId,
        prompt: &str,
    ) -> AppResult<Vec<ProjectSkill>> {
        let mut skills = Vec::new();
        for skill_id in extract_project_skill_directives(prompt) {
            let Some(skill) = self
                .repo
                .get_by_id(&ProjectSkillId::from_string(skill_id))
                .await?
            else {
                continue;
            };
            if &skill.project_id != project_id
                || skill.status != ProjectSkillLifecycleStatus::Approved
                || skill.archived
            {
                continue;
            }
            skills.push(skill);
        }
        Ok(skills)
    }

    pub async fn approve_skill(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
        self.repo
            .update_lifecycle_status(id, ProjectSkillLifecycleStatus::Approved)
            .await
    }

    pub async fn reject_skill(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
        self.repo
            .update_lifecycle_status(id, ProjectSkillLifecycleStatus::Rejected)
            .await
    }

    pub async fn archive_skill(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
        self.repo
            .update_lifecycle_status(id, ProjectSkillLifecycleStatus::Archived)
            .await
    }

    pub async fn pin_skill(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
        let Some(skill) = self.repo.get_by_id(id).await? else {
            return Ok(None);
        };
        if skill.archived || skill.status != ProjectSkillLifecycleStatus::Approved {
            return Err(AppError::Validation(
                "only approved active project skills can be pinned".to_string(),
            ));
        }
        self.repo.update_pinned(id, true).await
    }

    pub async fn unpin_skill(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
        self.repo.update_pinned(id, false).await
    }
}

impl ProjectSkillDistillerService {
    pub fn new(
        outcome_repo: Arc<dyn TaskOutcomeRepository>,
        project_skill_repo: Arc<dyn ProjectSkillRepository>,
    ) -> Self {
        Self {
            outcome_repo,
            skill_service: ProjectSkillService::new(project_skill_repo),
        }
    }

    pub async fn distill_eligible_outcomes(
        &self,
        input: DistillEligibleOutcomesInput,
    ) -> AppResult<DistillEligibleOutcomesResult> {
        let outcomes = self
            .outcome_repo
            .list_by_project(
                &input.project_id,
                TaskOutcomeListOptions {
                    source: input.source,
                    status: Some(TaskOutcomeStatus::Eligible),
                },
            )
            .await?;
        let existing_skills = self
            .skill_service
            .list_project_skills(
                &input.project_id,
                ProjectSkillListOptions {
                    include_archived: true,
                    ..Default::default()
                },
            )
            .await?;
        let existing_outcome_ids = existing_skills
            .iter()
            .filter_map(|skill| {
                skill
                    .provenance_json
                    .get("outcome_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect::<BTreeSet<_>>();

        let mut staged_skills = Vec::new();
        let mut skipped_existing = 0;
        let limit = input.limit.max(1);
        for outcome in outcomes {
            if staged_skills.len() >= limit {
                break;
            }
            if existing_outcome_ids.contains(outcome.id.as_str()) {
                skipped_existing += 1;
                continue;
            }
            if let Some(staged) = self
                .stage_eligible_outcome_candidate_with_origin(&outcome, input.origin)
                .await?
            {
                staged_skills.push(staged);
            } else {
                skipped_existing += 1;
            }
        }

        Ok(DistillEligibleOutcomesResult {
            staged_skills,
            skipped_existing,
        })
    }

    pub async fn stage_eligible_outcome_candidate(
        &self,
        outcome: &TaskOutcome,
    ) -> AppResult<Option<ProjectSkill>> {
        self.stage_eligible_outcome_candidate_with_origin(
            outcome,
            ProjectSkillDistillationOrigin::DeterministicService,
        )
        .await
    }

    pub async fn stage_eligible_outcome_candidate_with_origin(
        &self,
        outcome: &TaskOutcome,
        origin: ProjectSkillDistillationOrigin,
    ) -> AppResult<Option<ProjectSkill>> {
        if outcome.status != TaskOutcomeStatus::Eligible {
            return Ok(None);
        }
        let existing_skills = self
            .skill_service
            .list_project_skills(
                &outcome.project_id,
                ProjectSkillListOptions {
                    include_archived: true,
                    ..Default::default()
                },
            )
            .await?;
        let already_staged = existing_skills.iter().any(|skill| {
            skill
                .provenance_json
                .get("outcome_id")
                .and_then(Value::as_str)
                == Some(outcome.id.as_str())
        });
        if already_staged {
            return Ok(None);
        }

        let candidate = build_distilled_skill_candidate(outcome, origin);
        self.skill_service
            .stage_skill_from_outcome(candidate)
            .await
            .map(Some)
    }
}

pub struct SkillUsageService {
    repo: Arc<dyn SkillUsageEventRepository>,
}

pub struct ProjectSkillReportService {
    skill_repo: Arc<dyn ProjectSkillRepository>,
    usage_repo: Arc<dyn SkillUsageEventRepository>,
    outcome_repo: Arc<dyn TaskOutcomeRepository>,
}

pub struct ProjectSkillImportPreviewService {
    skill_repo: Arc<dyn ProjectSkillRepository>,
}

pub struct MemoryToProjectSkillPromotionService {
    memory_repo: Arc<dyn MemoryEntryRepository>,
    skill_service: ProjectSkillService,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillImportCandidate {
    pub external_id: Option<String>,
    pub title: String,
    pub bucket: String,
    pub stage: String,
    pub scope_paths: Vec<String>,
    pub compact_guidance: String,
    pub body_markdown: String,
    pub predicted_effect: String,
    pub provenance_json: Value,
    pub source_snapshot_json: Value,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillImportPreviewInput {
    pub project_id: ProjectId,
    pub candidates: Vec<ProjectSkillImportCandidate>,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillImportApplyInput {
    pub project_id: ProjectId,
    pub candidates: Vec<ProjectSkillImportCandidate>,
    pub confirm_import: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSkillImportDecision {
    Eligible,
    Invalid,
    Duplicate,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillImportPreviewRow {
    pub index: usize,
    pub external_id: Option<String>,
    pub title: String,
    pub decision: ProjectSkillImportDecision,
    pub reasons: Vec<String>,
    pub duplicate_project_skill_id: Option<ProjectSkillId>,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillImportPreview {
    pub rows: Vec<ProjectSkillImportPreviewRow>,
    pub eligible_count: usize,
    pub invalid_count: usize,
    pub duplicate_count: usize,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillImportApplyResult {
    pub preview: ProjectSkillImportPreview,
    pub imported_skills: Vec<ProjectSkill>,
}

#[derive(Debug, Clone)]
pub struct PromoteMemoryToProjectSkillInput {
    pub project_id: ProjectId,
    pub memory_id: MemoryEntryId,
    pub title: Option<String>,
    pub bucket: String,
    pub stage: String,
    pub compact_guidance: String,
    pub body_markdown: String,
    pub predicted_effect: String,
}

#[derive(Debug, Clone)]
pub struct PromoteMemoryToProjectSkillResult {
    pub skill: ProjectSkill,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillReportOptions {
    pub min_linked_outcomes: usize,
    pub stale_after_days: i64,
    pub now: DateTime<Utc>,
}

impl Default for ProjectSkillReportOptions {
    fn default() -> Self {
        Self {
            min_linked_outcomes: 5,
            stale_after_days: 30,
            now: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSkillAgingStatus {
    Active,
    Stale,
    Unused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectSkillEvidenceLevel {
    InsufficientData,
    Descriptive,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillReportCard {
    pub project_skill_id: ProjectSkillId,
    pub title: String,
    pub bucket: String,
    pub stage: String,
    pub pinned: bool,
    pub usage_count: usize,
    pub linked_outcome_count: usize,
    pub succeeded_outcome_count: usize,
    pub failed_outcome_count: usize,
    pub unknown_outcome_count: usize,
    pub last_used_at: Option<DateTime<Utc>>,
    pub age_days: i64,
    pub aging_status: ProjectSkillAgingStatus,
    pub evidence_level: ProjectSkillEvidenceLevel,
}

impl ProjectSkillReportService {
    pub fn new(
        skill_repo: Arc<dyn ProjectSkillRepository>,
        usage_repo: Arc<dyn SkillUsageEventRepository>,
        outcome_repo: Arc<dyn TaskOutcomeRepository>,
    ) -> Self {
        Self {
            skill_repo,
            usage_repo,
            outcome_repo,
        }
    }

    pub async fn list_report_cards(
        &self,
        project_id: &ProjectId,
        options: ProjectSkillReportOptions,
    ) -> AppResult<Vec<ProjectSkillReportCard>> {
        let skills = self
            .skill_repo
            .list_by_project(
                project_id,
                ProjectSkillListOptions {
                    status: Some(ProjectSkillLifecycleStatus::Approved),
                    include_archived: false,
                    ..Default::default()
                },
            )
            .await?;
        let outcomes = self
            .outcome_repo
            .list_by_project(project_id, TaskOutcomeListOptions::default())
            .await?
            .into_iter()
            .map(|outcome| (outcome.id.as_str().to_string(), outcome))
            .collect::<BTreeMap<_, _>>();

        let mut cards = Vec::with_capacity(skills.len());
        for skill in skills {
            let usage = self
                .usage_repo
                .list_by_project(
                    project_id,
                    SkillUsageListOptions {
                        project_skill_id: Some(skill.id.clone()),
                        agent_run_id: None,
                    },
                )
                .await?;
            let mut linked_outcome_count = 0;
            let mut succeeded_outcome_count = 0;
            let mut failed_outcome_count = 0;
            let mut unknown_outcome_count = 0;
            for event in &usage {
                let Some(outcome_id) = event.outcome_id.as_ref() else {
                    continue;
                };
                let Some(outcome) = outcomes.get(outcome_id.as_str()) else {
                    unknown_outcome_count += 1;
                    continue;
                };
                linked_outcome_count += 1;
                match outcome.status {
                    TaskOutcomeStatus::Succeeded => succeeded_outcome_count += 1,
                    TaskOutcomeStatus::Failed => failed_outcome_count += 1,
                    _ => unknown_outcome_count += 1,
                }
            }

            let last_used_at = usage.iter().map(|event| event.created_at).max();
            let age_start = last_used_at.unwrap_or(skill.created_at);
            let age_days = options
                .now
                .signed_duration_since(age_start)
                .num_days()
                .max(0);
            let aging_status = if skill.pinned {
                ProjectSkillAgingStatus::Active
            } else if usage.is_empty() && age_days >= options.stale_after_days {
                ProjectSkillAgingStatus::Unused
            } else if age_days >= options.stale_after_days {
                ProjectSkillAgingStatus::Stale
            } else {
                ProjectSkillAgingStatus::Active
            };
            let evidence_level = if linked_outcome_count >= options.min_linked_outcomes {
                ProjectSkillEvidenceLevel::Descriptive
            } else {
                ProjectSkillEvidenceLevel::InsufficientData
            };

            cards.push(ProjectSkillReportCard {
                project_skill_id: skill.id,
                title: skill.title,
                bucket: skill.bucket,
                stage: skill.stage,
                pinned: skill.pinned,
                usage_count: usage.len(),
                linked_outcome_count,
                succeeded_outcome_count,
                failed_outcome_count,
                unknown_outcome_count,
                last_used_at,
                age_days,
                aging_status,
                evidence_level,
            });
        }

        cards.sort_by(|left, right| {
            right
                .usage_count
                .cmp(&left.usage_count)
                .then_with(|| right.last_used_at.cmp(&left.last_used_at))
                .then_with(|| left.title.cmp(&right.title))
        });
        Ok(cards)
    }
}

impl ProjectSkillImportPreviewService {
    pub fn new(skill_repo: Arc<dyn ProjectSkillRepository>) -> Self {
        Self { skill_repo }
    }

    pub async fn preview_import(
        &self,
        input: ProjectSkillImportPreviewInput,
    ) -> AppResult<ProjectSkillImportPreview> {
        let existing_skills = self
            .skill_repo
            .list_by_project(
                &input.project_id,
                ProjectSkillListOptions {
                    include_archived: true,
                    ..Default::default()
                },
            )
            .await?;
        let mut existing_keys = BTreeMap::new();
        for skill in &existing_skills {
            for key in project_skill_import_keys(skill) {
                existing_keys.insert(key, skill.id.clone());
            }
        }

        let mut seen_keys = BTreeSet::new();
        let mut rows = Vec::with_capacity(input.candidates.len());
        let mut eligible_count = 0;
        let mut invalid_count = 0;
        let mut duplicate_count = 0;

        for (index, candidate) in input.candidates.into_iter().enumerate() {
            let mut reasons = validate_import_candidate(&candidate);
            let keys = candidate_import_keys(&candidate);
            let duplicate_project_skill_id =
                keys.iter().find_map(|key| existing_keys.get(key).cloned());
            if duplicate_project_skill_id.is_some() {
                reasons.push("matching project skill already exists".to_string());
            }
            if keys.iter().any(|key| seen_keys.contains(key)) {
                reasons.push("duplicate candidate in import manifest".to_string());
            }
            for key in keys {
                seen_keys.insert(key);
            }

            let decision = if duplicate_project_skill_id.is_some() {
                duplicate_count += 1;
                ProjectSkillImportDecision::Duplicate
            } else if reasons.is_empty() {
                eligible_count += 1;
                ProjectSkillImportDecision::Eligible
            } else {
                invalid_count += 1;
                ProjectSkillImportDecision::Invalid
            };

            rows.push(ProjectSkillImportPreviewRow {
                index,
                external_id: candidate.external_id,
                title: candidate.title,
                decision,
                reasons,
                duplicate_project_skill_id,
            });
        }

        Ok(ProjectSkillImportPreview {
            rows,
            eligible_count,
            invalid_count,
            duplicate_count,
        })
    }

    pub async fn apply_import(
        &self,
        input: ProjectSkillImportApplyInput,
    ) -> AppResult<ProjectSkillImportApplyResult> {
        if !input.confirm_import {
            return Err(AppError::Validation(
                "project skill import requires confirm_import=true".to_string(),
            ));
        }

        let preview = self
            .preview_import(ProjectSkillImportPreviewInput {
                project_id: input.project_id.clone(),
                candidates: input.candidates.clone(),
            })
            .await?;
        let eligible_indexes = preview
            .rows
            .iter()
            .filter(|row| row.decision == ProjectSkillImportDecision::Eligible)
            .map(|row| row.index)
            .collect::<BTreeSet<_>>();
        let skill_service = ProjectSkillService::new(Arc::clone(&self.skill_repo));
        let mut imported_skills = Vec::with_capacity(eligible_indexes.len());

        for (index, candidate) in input.candidates.into_iter().enumerate() {
            if !eligible_indexes.contains(&index) {
                continue;
            }
            let now = Utc::now();
            let skill = ProjectSkill {
                id: ProjectSkillId::new(),
                project_id: input.project_id.clone(),
                title: candidate.title,
                bucket: candidate.bucket,
                stage: candidate.stage,
                status: ProjectSkillLifecycleStatus::Staged,
                pinned: false,
                archived: false,
                scope_paths: candidate.scope_paths,
                compact_guidance: candidate.compact_guidance,
                body_markdown: candidate.body_markdown,
                predicted_effect: Some(candidate.predicted_effect),
                provenance_json: serde_json::json!({
                    "source": "project_skill_import",
                    "external_id": candidate.external_id,
                    "import_provenance": candidate.provenance_json,
                    "source_snapshot": candidate.source_snapshot_json,
                }),
                companion_of_skill_id: None,
                created_at: now,
                updated_at: now,
            };
            imported_skills.push(skill_service.stage_skill(skill).await?);
        }

        Ok(ProjectSkillImportApplyResult {
            preview,
            imported_skills,
        })
    }
}

impl MemoryToProjectSkillPromotionService {
    pub fn new(
        memory_repo: Arc<dyn MemoryEntryRepository>,
        project_skill_repo: Arc<dyn ProjectSkillRepository>,
    ) -> Self {
        Self {
            memory_repo,
            skill_service: ProjectSkillService::new(project_skill_repo),
        }
    }

    pub async fn promote_memory(
        &self,
        input: PromoteMemoryToProjectSkillInput,
    ) -> AppResult<PromoteMemoryToProjectSkillResult> {
        let memory = self
            .memory_repo
            .get_by_id(&input.memory_id)
            .await?
            .ok_or_else(|| AppError::Validation("memory entry not found".to_string()))?;
        validate_memory_promotion_boundary(&memory, &input)?;

        let now = Utc::now();
        let skill = ProjectSkill {
            id: ProjectSkillId::new(),
            project_id: input.project_id,
            title: input.title.unwrap_or_else(|| memory.title.clone()),
            bucket: input.bucket,
            stage: input.stage,
            status: ProjectSkillLifecycleStatus::Staged,
            pinned: false,
            archived: false,
            scope_paths: memory.scope_paths.clone(),
            compact_guidance: input.compact_guidance,
            body_markdown: input.body_markdown,
            predicted_effect: Some(input.predicted_effect),
            provenance_json: serde_json::json!({
                "source": "memory_to_project_skill_promotion",
                "memory_id": memory.id.as_str(),
                "memory_bucket": memory.bucket.to_string(),
                "memory_title": memory.title,
                "memory_summary": memory.summary,
                "source_context_type": memory.source_context_type,
                "source_context_id": memory.source_context_id,
                "source_conversation_id": memory.source_conversation_id,
            }),
            companion_of_skill_id: None,
            created_at: now,
            updated_at: now,
        };

        Ok(PromoteMemoryToProjectSkillResult {
            skill: self.skill_service.stage_skill(skill).await?,
        })
    }
}

impl SkillUsageService {
    pub fn new(repo: Arc<dyn SkillUsageEventRepository>) -> Self {
        Self { repo }
    }

    pub async fn record_usage(&self, event: SkillUsageEvent) -> AppResult<SkillUsageEvent> {
        validate_non_empty("skill usage injection_kind", &event.injection_kind)?;
        self.repo.record(event).await
    }

    pub async fn list_project_usage(
        &self,
        project_id: &ProjectId,
        options: SkillUsageListOptions,
    ) -> AppResult<Vec<SkillUsageEvent>> {
        self.repo.list_by_project(project_id, options).await
    }
}

pub fn new_empty_task_outcome(
    project_id: ProjectId,
    source: impl Into<String>,
    source_ref_kind: impl Into<String>,
    source_ref_id: impl Into<String>,
) -> TaskOutcome {
    let now = Utc::now();
    TaskOutcome {
        id: TaskOutcomeId::new(),
        project_id,
        source: source.into(),
        source_ref_kind: source_ref_kind.into(),
        source_ref_id: source_ref_id.into(),
        task_id: None,
        conversation_id: None,
        agent_run_id: None,
        pull_request_id: None,
        proposal_id: None,
        verification_id: None,
        review_id: None,
        outcome_class: None,
        status: TaskOutcomeStatus::Unknown,
        evidence_json: serde_json::json!({}),
        provider_harness: None,
        provider_session_id: None,
        created_at: now,
        updated_at: now,
    }
}

pub fn new_skill_usage_event(
    project_id: ProjectId,
    project_skill_id: ProjectSkillId,
    injection_kind: impl Into<String>,
) -> SkillUsageEvent {
    SkillUsageEvent {
        id: SkillUsageEventId::new(),
        project_id,
        project_skill_id,
        conversation_id: None,
        agent_run_id: None,
        provider_harness: None,
        stage: None,
        bucket: None,
        injection_kind: injection_kind.into(),
        outcome_id: None,
        metadata_json: serde_json::json!({}),
        created_at: Utc::now(),
    }
}

fn validate_project_skill(skill: &ProjectSkill) -> AppResult<()> {
    validate_non_empty("project skill title", &skill.title)?;
    validate_project_skill_bucket(&skill.bucket)?;
    validate_project_skill_stage(&skill.stage)?;
    validate_non_empty("project skill compact_guidance", &skill.compact_guidance)
}

fn validate_import_candidate(candidate: &ProjectSkillImportCandidate) -> Vec<String> {
    let mut reasons = Vec::new();
    for (label, value) in [
        ("title", candidate.title.as_str()),
        ("bucket", candidate.bucket.as_str()),
        ("stage", candidate.stage.as_str()),
        ("compact_guidance", candidate.compact_guidance.as_str()),
        ("body_markdown", candidate.body_markdown.as_str()),
        ("predicted_effect", candidate.predicted_effect.as_str()),
    ] {
        if value.trim().is_empty() {
            reasons.push(format!("{label} is required"));
        }
    }

    if !is_project_skill_bucket(candidate.bucket.as_str()) {
        reasons.push(format!(
            "bucket must be one of {}",
            PROJECT_SKILL_BUCKET_VALUES.join(", ")
        ));
    }
    if !is_project_skill_stage(candidate.stage.as_str()) {
        reasons.push(format!(
            "stage must be one of {}",
            PROJECT_SKILL_STAGE_VALUES.join(", ")
        ));
    }

    if !candidate.provenance_json.is_object()
        || candidate
            .provenance_json
            .as_object()
            .map(|object| object.is_empty())
            .unwrap_or(true)
    {
        reasons.push("provenance is required".to_string());
    }

    if !candidate.source_snapshot_json.is_object()
        || candidate
            .source_snapshot_json
            .as_object()
            .map(|object| object.is_empty())
            .unwrap_or(true)
    {
        reasons.push("source snapshot is required before import".to_string());
    }

    for path in &candidate.scope_paths {
        if !is_safe_import_scope_path(path) {
            reasons.push(format!("invalid scope path: {path}"));
        }
    }

    reasons
}

fn validate_memory_promotion_boundary(
    memory: &MemoryEntry,
    input: &PromoteMemoryToProjectSkillInput,
) -> AppResult<()> {
    if memory.project_id != input.project_id {
        return Err(AppError::Validation(
            "memory entry belongs to a different project".to_string(),
        ));
    }
    if memory.status != MemoryStatus::Active {
        return Err(AppError::Validation(
            "only active memory entries can be promoted".to_string(),
        ));
    }
    validate_project_skill_bucket(&input.bucket)?;
    validate_project_skill_stage(&input.stage)?;
    validate_non_empty("project skill compact_guidance", &input.compact_guidance)?;
    validate_non_empty("project skill body_markdown", &input.body_markdown)?;
    validate_non_empty("predicted_effect", &input.predicted_effect)?;

    if input.compact_guidance.trim() == memory.summary.trim()
        || input.body_markdown.trim() == memory.details_markdown.trim()
    {
        return Err(AppError::Validation(
            "memory promotion requires procedural guidance distinct from factual memory content"
                .to_string(),
        ));
    }
    Ok(())
}

fn validate_project_skill_bucket(bucket: &str) -> AppResult<()> {
    validate_non_empty("project skill bucket", bucket)?;
    if !is_project_skill_bucket(bucket) {
        return Err(AppError::Validation(format!(
            "project skill bucket must be one of {}",
            PROJECT_SKILL_BUCKET_VALUES.join(", ")
        )));
    }
    Ok(())
}

fn validate_project_skill_stage(stage: &str) -> AppResult<()> {
    validate_non_empty("project skill stage", stage)?;
    if !is_project_skill_stage(stage) {
        return Err(AppError::Validation(format!(
            "project skill stage must be one of {}",
            PROJECT_SKILL_STAGE_VALUES.join(", ")
        )));
    }
    Ok(())
}

fn is_project_skill_bucket(bucket: &str) -> bool {
    PROJECT_SKILL_BUCKET_VALUES.contains(&bucket.trim())
}

fn is_project_skill_stage(stage: &str) -> bool {
    PROJECT_SKILL_STAGE_VALUES.contains(&stage.trim())
}

fn project_skill_import_keys(skill: &ProjectSkill) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(external_id) = project_skill_import_external_id(skill) {
        keys.push(normalized_source_import_key(&external_id));
    }
    keys.push(normalized_title_import_key(
        &skill.title,
        &skill.bucket,
        &skill.stage,
    ));
    keys
}

fn candidate_import_keys(candidate: &ProjectSkillImportCandidate) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(external_id) = candidate
        .external_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        keys.push(normalized_source_import_key(external_id));
    }
    keys.push(normalized_title_import_key(
        &candidate.title,
        &candidate.bucket,
        &candidate.stage,
    ));
    keys
}

fn project_skill_import_external_id(skill: &ProjectSkill) -> Option<String> {
    skill
        .provenance_json
        .get("external_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalized_source_import_key(external_id: &str) -> String {
    format!("source:{}", external_id.trim().to_lowercase())
}

fn normalized_title_import_key(title: &str, bucket: &str, stage: &str) -> String {
    format!(
        "title:{}\n{}\n{}",
        title.trim().to_lowercase(),
        bucket.trim().to_lowercase(),
        stage.trim().to_lowercase()
    )
}

fn set_project_skill_source_sync_enabled(provenance: &mut Value, enabled: bool) {
    if !provenance.is_object() {
        *provenance = serde_json::json!({});
    }
    if let Some(object) = provenance.as_object_mut() {
        object.insert("source_sync_enabled".to_string(), Value::Bool(enabled));
        if let Some(source_snapshot) = object
            .get_mut("source_snapshot")
            .and_then(Value::as_object_mut)
        {
            source_snapshot.insert("source_sync_enabled".to_string(), Value::Bool(enabled));
        }
    }
}

fn is_safe_import_scope_path(path: &str) -> bool {
    let path = path.trim();
    !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('~')
        && !path.contains('\\')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn validate_non_empty(label: &str, value: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        return Err(AppError::Validation(format!("{label} is required")));
    }
    Ok(())
}

fn build_distilled_skill_candidate(
    outcome: &TaskOutcome,
    origin: ProjectSkillDistillationOrigin,
) -> StageProjectSkillFromOutcomeInput {
    if outcome.source == "git_commit_history" {
        return build_git_commit_skill_candidate(outcome, origin);
    }

    let outcome_class = outcome
        .outcome_class
        .as_deref()
        .unwrap_or("unknown_outcome");
    let readable_class = humanize_identifier(outcome_class);
    let bucket = bucket_for_outcome_source(&outcome.source).to_string();
    let stage = stage_for_outcome_source(&outcome.source).to_string();
    let evidence_summary =
        serde_json::to_string(&outcome.evidence_json).unwrap_or_else(|_| "{}".to_string());
    let evidence_summary = truncate_for_skill_body(&evidence_summary, 1200);

    StageProjectSkillFromOutcomeInput {
        outcome: outcome.clone(),
        title: format!("Avoid repeat {readable_class}"),
        bucket,
        stage,
        scope_paths: scope_paths_from_outcome(outcome),
        compact_guidance: format!(
            "Before similar work, check prior {readable_class} evidence and avoid repeating the same failure pattern."
        ),
        body_markdown: format!(
            "## Learned candidate\n\nPrior outcome `{}` from `{}` was marked eligible for review.\n\nEvidence summary:\n\n```json\n{}\n```",
            outcome_class,
            outcome.source,
            evidence_summary
        ),
        predicted_effect: format!(
            "Reduces repeat {readable_class} outcomes by surfacing the prior evidence before similar work."
        ),
        additional_provenance: serde_json::json!({
            "distiller": "deterministic_eligible_outcome_v1",
            "distillation_origin": origin.as_str(),
            "pipeline_role": origin.pipeline_role(),
        }),
    }
}

fn build_git_commit_skill_candidate(
    outcome: &TaskOutcome,
    origin: ProjectSkillDistillationOrigin,
) -> StageProjectSkillFromOutcomeInput {
    let subject = outcome
        .evidence_json
        .get("subject")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("recent repository change");
    let title_subject = truncate_for_skill_title(subject, 72);
    let evidence_summary =
        serde_json::to_string(&outcome.evidence_json).unwrap_or_else(|_| "{}".to_string());
    let evidence_summary = truncate_for_skill_body(&evidence_summary, 1200);

    StageProjectSkillFromOutcomeInput {
        outcome: outcome.clone(),
        title: format!("Review reusable commit pattern: {title_subject}"),
        bucket: "execution".to_string(),
        stage: "execution".to_string(),
        scope_paths: scope_paths_from_outcome(outcome),
        compact_guidance: format!(
            "Before similar code work, review the prior commit `{title_subject}` and extract only the reusable procedure that applies to this project."
        ),
        body_markdown: format!(
            "## Candidate from git history\n\nRalphX found this recent commit while scanning project history. Edit this draft before approval so it describes a reusable agent procedure, not just a past change.\n\nCommit evidence:\n\n```json\n{}\n```",
            evidence_summary
        ),
        predicted_effect: format!(
            "Helps agents reuse the implementation pattern behind `{title_subject}` without rereading the full repository history."
        ),
        additional_provenance: serde_json::json!({
            "distiller": "git_history_commit_v1",
            "distillation_origin": origin.as_str(),
            "pipeline_role": origin.pipeline_role(),
        }),
    }
}

fn bucket_for_outcome_source(source: &str) -> &'static str {
    match source {
        "review" | "github_pr_review" => "review",
        "merge" | "merge_validation" => "merge",
        "verification" | "plan_mode" => "planning",
        "agent_session" | "task_pipeline" => "execution",
        _ => "execution",
    }
}

fn stage_for_outcome_source(source: &str) -> &'static str {
    match source {
        "review" | "github_pr_review" => "review",
        "merge" | "merge_validation" => "merge",
        "verification" | "plan_mode" => "planning",
        _ => "execution",
    }
}

fn scope_paths_from_outcome(outcome: &TaskOutcome) -> Vec<String> {
    outcome
        .evidence_json
        .get("scope_paths")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn humanize_identifier(value: &str) -> String {
    value.replace(['_', '-'], " ")
}

fn truncate_for_skill_body(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn truncate_for_skill_title(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut truncated = trimmed.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn extract_project_skill_directives(text: &str) -> Vec<String> {
    let mut skill_ids = BTreeSet::new();
    for line in text.lines() {
        if let Some(index) = line.find("ralphx_project_skill=") {
            let raw = &line[index + "ralphx_project_skill=".len()..];
            if let Some(value) = raw.split_whitespace().next() {
                let skill_id = value
                    .trim_matches(|char| matches!(char, '<' | '>' | '-' | '"' | '\'' | ';' | ','));
                if is_safe_project_skill_id(skill_id) {
                    skill_ids.insert(skill_id.to_string());
                }
            }
        }
    }
    skill_ids.into_iter().collect()
}

fn is_safe_project_skill_id(skill_id: &str) -> bool {
    !skill_id.is_empty()
        && skill_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn project_skill_to_constraint_citation(skill: ProjectSkill) -> LearnedSkillConstraintCitation {
    LearnedSkillConstraintCitation {
        skill_id: skill.id.as_str().to_string(),
        title: skill.title,
        predicted_effect: skill.predicted_effect.unwrap_or_default(),
        compact_guidance: skill.compact_guidance,
        provenance_refs: provenance_refs_from_json(&skill.provenance_json),
    }
}

fn provenance_refs_from_json(value: &Value) -> Vec<String> {
    for key in ["provenance_refs", "refs"] {
        if let Some(refs) = value.get(key).and_then(Value::as_array) {
            return refs
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect();
        }
    }
    value
        .get("source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| vec![value.to_string()])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;

    use chrono::{Duration, Utc};
    use serde_json::json;

    use super::{
        new_empty_task_outcome, new_skill_usage_event, DistillEligibleOutcomesInput,
        MemoryToProjectSkillPromotionService, ProjectSkillAgingStatus,
        ProjectSkillDistillationOrigin, ProjectSkillDistillerService, ProjectSkillEvidenceLevel,
        ProjectSkillImportApplyInput, ProjectSkillImportCandidate, ProjectSkillImportDecision,
        ProjectSkillImportPreviewInput, ProjectSkillImportPreviewService,
        ProjectSkillReportOptions, ProjectSkillReportService, ProjectSkillService,
        PromoteMemoryToProjectSkillInput, SkillUsageService, StageProjectSkillFromOutcomeInput,
        UpdateProjectSkillContentInput,
    };
    use crate::domain::entities::types::ProjectId;
    use crate::domain::entities::{
        MemoryBucket, MemoryEntry, MemoryEntryId, ProjectSkill, ProjectSkillId,
        ProjectSkillLifecycleStatus, SkillUsageEventId, TaskOutcomeId, TaskOutcomeStatus,
    };
    use crate::domain::repositories::{
        MemoryEntryRepository, ProjectSkillListOptions, ProjectSkillRepository,
        SkillUsageEventRepository, SkillUsageListOptions, TaskOutcomeRepository,
        UpsertTaskOutcomeInput,
    };
    use crate::infrastructure::memory::{
        InMemoryMemoryEntryRepository, MemoryProjectSkillRepository,
        MemorySkillUsageEventRepository, MemoryTaskOutcomeRepository,
    };

    fn staged_skill(project_id: ProjectId) -> ProjectSkill {
        let now = Utc::now();
        ProjectSkill {
            id: ProjectSkillId::new(),
            project_id,
            title: "Keep learned skills repository-backed".to_string(),
            bucket: "execution".to_string(),
            stage: "execution".to_string(),
            status: ProjectSkillLifecycleStatus::Staged,
            pinned: false,
            archived: false,
            scope_paths: Vec::new(),
            compact_guidance: "Read approved learned skills from the project skill service."
                .to_string(),
            body_markdown: "Detailed guidance".to_string(),
            predicted_effect: Some("Avoids adapter-only injection.".to_string()),
            provenance_json: json!({ "test": true }),
            companion_of_skill_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn import_candidate() -> ProjectSkillImportCandidate {
        ProjectSkillImportCandidate {
            external_id: Some("manifest-skill-1".to_string()),
            title: "Keep learned skills repository-backed".to_string(),
            bucket: "execution".to_string(),
            stage: "execution".to_string(),
            scope_paths: vec!["src-tauri/src/domain/services".to_string()],
            compact_guidance: "Read approved learned skills from the project skill service."
                .to_string(),
            body_markdown: "Detailed guidance".to_string(),
            predicted_effect: "Avoids adapter-only injection.".to_string(),
            provenance_json: json!({
                "source": "external_manifest",
                "source_ref": "manifest-skill-1"
            }),
            source_snapshot_json: json!({
                "kind": "project_skill_manifest",
                "captured_at": "2026-06-15T00:00:00Z",
                "sha256": "test-snapshot"
            }),
        }
    }

    #[test]
    fn learned_skill_entity_ids_and_statuses_round_trip() {
        let task_outcome_id = TaskOutcomeId::from_string("outcome-1");
        let project_skill_id = ProjectSkillId::from_string("skill-1");
        let usage_event_id = SkillUsageEventId::from_string("usage-1");

        assert_eq!(task_outcome_id.as_str(), "outcome-1");
        assert_eq!(project_skill_id.as_str(), "skill-1");
        assert_eq!(usage_event_id.as_str(), "usage-1");
        assert!(!TaskOutcomeId::new().as_str().is_empty());
        assert!(!ProjectSkillId::new().as_str().is_empty());
        assert!(!SkillUsageEventId::new().as_str().is_empty());

        for (value, expected) in [
            ("unknown", TaskOutcomeStatus::Unknown),
            ("eligible", TaskOutcomeStatus::Eligible),
            ("ineligible", TaskOutcomeStatus::Ineligible),
            ("succeeded", TaskOutcomeStatus::Succeeded),
            ("failed", TaskOutcomeStatus::Failed),
        ] {
            assert_eq!(TaskOutcomeStatus::from_str(value).unwrap(), expected);
            assert_eq!(expected.to_string(), value);
        }
        assert!(TaskOutcomeStatus::from_str("pending").is_err());

        for (value, expected) in [
            ("staged", ProjectSkillLifecycleStatus::Staged),
            ("approved", ProjectSkillLifecycleStatus::Approved),
            ("rejected", ProjectSkillLifecycleStatus::Rejected),
            ("archived", ProjectSkillLifecycleStatus::Archived),
            ("retired", ProjectSkillLifecycleStatus::Retired),
        ] {
            assert_eq!(
                ProjectSkillLifecycleStatus::from_str(value).unwrap(),
                expected
            );
            assert_eq!(expected.to_string(), value);
        }
        assert!(ProjectSkillLifecycleStatus::from_str("draft").is_err());
    }

    fn promotion_input(
        project_id: ProjectId,
        memory_id: MemoryEntryId,
    ) -> PromoteMemoryToProjectSkillInput {
        PromoteMemoryToProjectSkillInput {
            project_id,
            memory_id,
            title: Some("Run branch checks before export".to_string()),
            bucket: "review".to_string(),
            stage: "review".to_string(),
            compact_guidance: "Before exporting learned skills, verify the checkout is on a clean review branch.".to_string(),
            body_markdown: "## Procedure\n\nCheck branch protection, worktree status, and preview output before export.".to_string(),
            predicted_effect: "Prevents unsafe direct writes during skill export.".to_string(),
        }
    }

    #[tokio::test]
    async fn project_skill_service_enforces_predicted_effect_before_staging() {
        let service = ProjectSkillService::new(Arc::new(MemoryProjectSkillRepository::new()));
        let project_id = ProjectId::from_string("project-1".to_string());
        let mut skill = staged_skill(project_id);
        skill.predicted_effect = None;

        let result = service.stage_skill(skill).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn project_skill_service_rejects_unknown_bucket_and_stage() {
        let service = ProjectSkillService::new(Arc::new(MemoryProjectSkillRepository::new()));
        let project_id = ProjectId::from_string("project-1".to_string());
        let mut skill = staged_skill(project_id);
        skill.bucket = "reviewer".to_string();

        let error = service
            .stage_skill(skill)
            .await
            .expect_err("unknown bucket/stage should be rejected");

        assert!(error
            .to_string()
            .contains("project skill bucket must be one of"));
    }

    #[tokio::test]
    async fn project_skill_service_rejects_unknown_stage() {
        let service = ProjectSkillService::new(Arc::new(MemoryProjectSkillRepository::new()));
        let project_id = ProjectId::from_string("project-1".to_string());
        let mut skill = staged_skill(project_id);
        skill.stage = "unsupported_stage".to_string();

        let error = service
            .stage_skill(skill)
            .await
            .expect_err("unknown stage should be rejected");

        assert!(error
            .to_string()
            .contains("project skill stage must be one of"));
    }

    #[tokio::test]
    async fn project_skill_service_rejects_unknown_category_on_update() {
        let repo = Arc::new(MemoryProjectSkillRepository::new());
        let service = ProjectSkillService::new(repo.clone());
        let project_id = ProjectId::from_string("project-1".to_string());
        let skill = repo.create(staged_skill(project_id)).await.unwrap();

        let error = service
            .update_skill_content(UpdateProjectSkillContentInput {
                project_skill_id: skill.id,
                title: "Updated skill".to_string(),
                bucket: "execution".to_string(),
                stage: "unsupported_stage".to_string(),
                scope_paths: Vec::new(),
                compact_guidance: "Updated compact guidance.".to_string(),
                body_markdown: "Updated body markdown.".to_string(),
                predicted_effect: "Keeps category validation centralized.".to_string(),
                source_sync_enabled: None,
            })
            .await
            .expect_err("unknown stage should be rejected during update");

        assert!(error
            .to_string()
            .contains("project skill stage must be one of"));
    }

    #[tokio::test]
    async fn project_skill_service_lifecycle_and_usage_services_work_together() {
        let skill_repo: Arc<dyn ProjectSkillRepository> =
            Arc::new(MemoryProjectSkillRepository::new());
        let usage_repo = Arc::new(MemorySkillUsageEventRepository::new());
        let skill_service = ProjectSkillService::new(skill_repo);
        let usage_service = SkillUsageService::new(usage_repo);
        let project_id = ProjectId::from_string("project-1".to_string());

        let staged = skill_service
            .stage_skill(staged_skill(project_id.clone()))
            .await
            .unwrap();
        let approved = skill_service
            .approve_skill(&staged.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(approved.status, ProjectSkillLifecycleStatus::Approved);

        let listed = skill_service
            .list_project_skills(
                &project_id,
                ProjectSkillListOptions {
                    status: Some(ProjectSkillLifecycleStatus::Approved),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);

        let pinned = skill_service
            .pin_skill(&approved.id)
            .await
            .unwrap()
            .unwrap();
        assert!(pinned.pinned);

        let unpinned = skill_service
            .unpin_skill(&approved.id)
            .await
            .unwrap()
            .unwrap();
        assert!(!unpinned.pinned);

        usage_service
            .record_usage(new_skill_usage_event(
                project_id.clone(),
                approved.id,
                "compact_index",
            ))
            .await
            .unwrap();
        let usage = usage_service
            .list_project_usage(&project_id, SkillUsageListOptions::default())
            .await
            .unwrap();
        assert_eq!(usage.len(), 1);
    }

    #[tokio::test]
    async fn project_skill_import_preview_marks_valid_rows_eligible_without_writing() {
        let repo = Arc::new(MemoryProjectSkillRepository::new());
        let service = ProjectSkillImportPreviewService::new(repo.clone());
        let project_id = ProjectId::from_string("project-import".to_string());

        let preview = service
            .preview_import(ProjectSkillImportPreviewInput {
                project_id: project_id.clone(),
                candidates: vec![import_candidate()],
            })
            .await
            .unwrap();

        assert_eq!(preview.eligible_count, 1);
        assert_eq!(preview.invalid_count, 0);
        assert_eq!(preview.duplicate_count, 0);
        assert_eq!(
            preview.rows[0].decision,
            ProjectSkillImportDecision::Eligible
        );
        let written = repo
            .list_by_project(&project_id, ProjectSkillListOptions::default())
            .await
            .unwrap();
        assert!(written.is_empty(), "preview must not write imported skills");
    }

    #[tokio::test]
    async fn project_skill_import_preview_fails_closed_for_invalid_manifest_and_paths() {
        let repo = Arc::new(MemoryProjectSkillRepository::new());
        let service = ProjectSkillImportPreviewService::new(repo);
        let project_id = ProjectId::from_string("project-import".to_string());
        let mut candidate = import_candidate();
        candidate.bucket = "reviewer".to_string();
        candidate.stage = "unsupported_stage".to_string();
        candidate.predicted_effect = " ".to_string();
        candidate.provenance_json = json!({});
        candidate.source_snapshot_json = json!(null);
        candidate.scope_paths = vec![
            "../outside".to_string(),
            "/absolute/path".to_string(),
            "src//bad".to_string(),
        ];

        let preview = service
            .preview_import(ProjectSkillImportPreviewInput {
                project_id,
                candidates: vec![candidate],
            })
            .await
            .unwrap();

        assert_eq!(preview.eligible_count, 0);
        assert_eq!(preview.invalid_count, 1);
        assert_eq!(
            preview.rows[0].decision,
            ProjectSkillImportDecision::Invalid
        );
        assert!(preview.rows[0]
            .reasons
            .iter()
            .any(|reason| reason == "predicted_effect is required"));
        assert!(preview.rows[0]
            .reasons
            .iter()
            .any(|reason| reason.starts_with("bucket must be one of")));
        assert!(preview.rows[0]
            .reasons
            .iter()
            .any(|reason| reason.starts_with("stage must be one of")));
        assert!(preview.rows[0]
            .reasons
            .iter()
            .any(|reason| reason == "provenance is required"));
        assert!(preview.rows[0]
            .reasons
            .iter()
            .any(|reason| reason == "source snapshot is required before import"));
        assert!(preview.rows[0]
            .reasons
            .iter()
            .any(|reason| reason.starts_with("invalid scope path: ../outside")));
    }

    #[tokio::test]
    async fn project_skill_import_preview_detects_existing_duplicates() {
        let repo = Arc::new(MemoryProjectSkillRepository::new());
        let project_id = ProjectId::from_string("project-import".to_string());
        let existing = repo.create(staged_skill(project_id.clone())).await.unwrap();
        let service = ProjectSkillImportPreviewService::new(repo);

        let preview = service
            .preview_import(ProjectSkillImportPreviewInput {
                project_id,
                candidates: vec![import_candidate()],
            })
            .await
            .unwrap();

        assert_eq!(preview.eligible_count, 0);
        assert_eq!(preview.duplicate_count, 1);
        assert_eq!(
            preview.rows[0].decision,
            ProjectSkillImportDecision::Duplicate
        );
        assert_eq!(
            preview.rows[0].duplicate_project_skill_id,
            Some(existing.id)
        );
        assert!(preview.rows[0]
            .reasons
            .iter()
            .any(|reason| reason == "matching project skill already exists"));
    }

    #[tokio::test]
    async fn project_skill_import_preview_dedupes_stable_source_id_before_title() {
        let repo = Arc::new(MemoryProjectSkillRepository::new());
        let project_id = ProjectId::from_string("project-import".to_string());
        let mut existing = staged_skill(project_id.clone());
        existing.title = "Old imported title".to_string();
        existing.provenance_json = json!({
            "source": "project_skill_import",
            "external_id": ".claude/skills/review/SKILL.md",
            "source_snapshot": {
                "relative_path": ".claude/skills/review/SKILL.md",
                "source_sync_enabled": true
            }
        });
        let existing = repo.create(existing).await.unwrap();
        let service = ProjectSkillImportPreviewService::new(repo);
        let mut candidate = import_candidate();
        candidate.external_id = Some(".claude/skills/review/SKILL.md".to_string());
        candidate.title = "New imported title".to_string();

        let preview = service
            .preview_import(ProjectSkillImportPreviewInput {
                project_id,
                candidates: vec![candidate],
            })
            .await
            .unwrap();

        assert_eq!(preview.eligible_count, 0);
        assert_eq!(preview.duplicate_count, 1);
        assert_eq!(
            preview.rows[0].decision,
            ProjectSkillImportDecision::Duplicate
        );
        assert_eq!(
            preview.rows[0].duplicate_project_skill_id,
            Some(existing.id)
        );
    }

    #[tokio::test]
    async fn project_skill_import_apply_requires_confirmation() {
        let repo = Arc::new(MemoryProjectSkillRepository::new());
        let service = ProjectSkillImportPreviewService::new(repo);
        let project_id = ProjectId::from_string("project-import".to_string());

        let result = service
            .apply_import(ProjectSkillImportApplyInput {
                project_id,
                candidates: vec![import_candidate()],
                confirm_import: false,
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn project_skill_import_apply_stages_only_eligible_rows_with_snapshot_provenance() {
        let repo = Arc::new(MemoryProjectSkillRepository::new());
        let service = ProjectSkillImportPreviewService::new(repo.clone());
        let project_id = ProjectId::from_string("project-import".to_string());
        let mut invalid = import_candidate();
        invalid.external_id = Some("invalid-skill".to_string());
        invalid.title = "Invalid imported skill".to_string();
        invalid.scope_paths = vec!["../outside".to_string()];

        let result = service
            .apply_import(ProjectSkillImportApplyInput {
                project_id: project_id.clone(),
                candidates: vec![import_candidate(), invalid],
                confirm_import: true,
            })
            .await
            .unwrap();

        assert_eq!(result.preview.eligible_count, 1);
        assert_eq!(result.preview.invalid_count, 1);
        assert_eq!(result.imported_skills.len(), 1);
        let imported = &result.imported_skills[0];
        assert_eq!(imported.status, ProjectSkillLifecycleStatus::Staged);
        assert_eq!(
            imported
                .provenance_json
                .get("source")
                .and_then(serde_json::Value::as_str),
            Some("project_skill_import")
        );
        assert!(imported.provenance_json.get("source_snapshot").is_some());

        let written = repo
            .list_by_project(&project_id, ProjectSkillListOptions::default())
            .await
            .unwrap();
        assert_eq!(written.len(), 1);
    }

    #[tokio::test]
    async fn memory_to_project_skill_promotion_requires_procedural_content() {
        let memory_repo = Arc::new(InMemoryMemoryEntryRepository::new());
        let skill_repo: Arc<dyn ProjectSkillRepository> =
            Arc::new(MemoryProjectSkillRepository::new());
        let project_id = ProjectId::from_string("project-memory".to_string());
        let memory = MemoryEntry::new(
            project_id.clone(),
            MemoryBucket::OperationalPlaybooks,
            "Export branch checks".to_string(),
            "Check the export branch before writing files.".to_string(),
            "Memory facts about the branch checks.".to_string(),
            vec!["src-tauri".to_string()],
            "hash-1".to_string(),
        );
        let memory = memory_repo.create(memory).await.unwrap();
        let service = MemoryToProjectSkillPromotionService::new(memory_repo, skill_repo);
        let mut input = promotion_input(project_id, memory.id);
        input.compact_guidance = "Check the export branch before writing files.".to_string();

        let result = service.promote_memory(input).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn memory_to_project_skill_promotion_rejects_unknown_category() {
        let memory_repo = Arc::new(InMemoryMemoryEntryRepository::new());
        let skill_repo: Arc<dyn ProjectSkillRepository> =
            Arc::new(MemoryProjectSkillRepository::new());
        let project_id = ProjectId::from_string("project-memory".to_string());
        let memory = MemoryEntry::new(
            project_id.clone(),
            MemoryBucket::OperationalPlaybooks,
            "Export branch checks".to_string(),
            "Check the export branch before writing files.".to_string(),
            "Memory facts about the branch checks.".to_string(),
            vec!["src-tauri".to_string()],
            "hash-1".to_string(),
        );
        let memory = memory_repo.create(memory).await.unwrap();
        let service = MemoryToProjectSkillPromotionService::new(memory_repo, skill_repo);
        let mut input = promotion_input(project_id, memory.id);
        input.stage = "unsupported_stage".to_string();

        let error = service
            .promote_memory(input)
            .await
            .expect_err("unknown stage should be rejected");

        assert!(error
            .to_string()
            .contains("project skill stage must be one of"));
    }

    #[tokio::test]
    async fn memory_to_project_skill_promotion_stages_skill_with_memory_provenance() {
        let memory_repo = Arc::new(InMemoryMemoryEntryRepository::new());
        let skill_repo: Arc<dyn ProjectSkillRepository> =
            Arc::new(MemoryProjectSkillRepository::new());
        let project_id = ProjectId::from_string("project-memory".to_string());
        let mut memory = MemoryEntry::new(
            project_id.clone(),
            MemoryBucket::OperationalPlaybooks,
            "Export branch checks".to_string(),
            "Check the export branch before writing files.".to_string(),
            "Memory facts about the branch checks.".to_string(),
            vec!["src-tauri".to_string()],
            "hash-1".to_string(),
        );
        memory.source_context_type = Some("agent_run".to_string());
        memory.source_context_id = Some("run-1".to_string());
        let memory = memory_repo.create(memory).await.unwrap();
        let service =
            MemoryToProjectSkillPromotionService::new(memory_repo, Arc::clone(&skill_repo));

        let result = service
            .promote_memory(promotion_input(project_id.clone(), memory.id.clone()))
            .await
            .unwrap();

        assert_eq!(result.skill.status, ProjectSkillLifecycleStatus::Staged);
        assert_eq!(result.skill.scope_paths, vec!["src-tauri".to_string()]);
        assert_eq!(
            result
                .skill
                .provenance_json
                .get("source")
                .and_then(serde_json::Value::as_str),
            Some("memory_to_project_skill_promotion")
        );
        assert_eq!(
            result
                .skill
                .provenance_json
                .get("memory_id")
                .and_then(serde_json::Value::as_str),
            Some(memory.id.as_str())
        );

        let written = skill_repo
            .list_by_project(&project_id, ProjectSkillListOptions::default())
            .await
            .unwrap();
        assert_eq!(written.len(), 1);
    }

    #[tokio::test]
    async fn project_skill_report_cards_are_descriptive_until_min_n_is_met() {
        let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
        let usage_repo = Arc::new(MemorySkillUsageEventRepository::new());
        let outcome_repo = Arc::new(MemoryTaskOutcomeRepository::new());
        let project_id = ProjectId::from_string("project-1".to_string());
        let now = Utc::now();
        let mut skill = staged_skill(project_id.clone());
        skill.status = ProjectSkillLifecycleStatus::Approved;
        skill.created_at = now - Duration::days(10);
        let skill = skill_repo.create(skill).await.unwrap();

        let mut success =
            new_empty_task_outcome(project_id.clone(), "review", "review_note", "review-1");
        success.status = TaskOutcomeStatus::Succeeded;
        outcome_repo
            .upsert(UpsertTaskOutcomeInput {
                outcome: success.clone(),
            })
            .await
            .unwrap();
        let mut failure =
            new_empty_task_outcome(project_id.clone(), "review", "review_note", "review-2");
        failure.status = TaskOutcomeStatus::Failed;
        outcome_repo
            .upsert(UpsertTaskOutcomeInput {
                outcome: failure.clone(),
            })
            .await
            .unwrap();

        for (index, outcome_id) in [Some(success.id.clone()), Some(failure.id.clone()), None]
            .into_iter()
            .enumerate()
        {
            let mut event =
                new_skill_usage_event(project_id.clone(), skill.id.clone(), "compact_index");
            event.outcome_id = outcome_id;
            event.created_at = now - Duration::days(index as i64);
            usage_repo.record(event).await.unwrap();
        }

        let service = ProjectSkillReportService::new(skill_repo, usage_repo, outcome_repo);
        let cards = service
            .list_report_cards(
                &project_id,
                ProjectSkillReportOptions {
                    min_linked_outcomes: 3,
                    stale_after_days: 30,
                    now,
                },
            )
            .await
            .unwrap();

        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(card.usage_count, 3);
        assert_eq!(card.linked_outcome_count, 2);
        assert_eq!(card.succeeded_outcome_count, 1);
        assert_eq!(card.failed_outcome_count, 1);
        assert_eq!(
            card.evidence_level,
            ProjectSkillEvidenceLevel::InsufficientData
        );
        assert_eq!(card.aging_status, ProjectSkillAgingStatus::Active);
    }

    #[tokio::test]
    async fn project_skill_report_cards_mark_unused_but_exempt_pinned_skills() {
        let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
        let usage_repo = Arc::new(MemorySkillUsageEventRepository::new());
        let outcome_repo = Arc::new(MemoryTaskOutcomeRepository::new());
        let project_id = ProjectId::from_string("project-1".to_string());
        let now = Utc::now();

        let mut unused = staged_skill(project_id.clone());
        unused.title = "Unused approved skill".to_string();
        unused.status = ProjectSkillLifecycleStatus::Approved;
        unused.created_at = now - Duration::days(45);
        skill_repo.create(unused).await.unwrap();

        let mut pinned = staged_skill(project_id.clone());
        pinned.title = "Pinned approved skill".to_string();
        pinned.status = ProjectSkillLifecycleStatus::Approved;
        pinned.pinned = true;
        pinned.created_at = now - Duration::days(45);
        skill_repo.create(pinned).await.unwrap();

        let service = ProjectSkillReportService::new(skill_repo, usage_repo, outcome_repo);
        let cards = service
            .list_report_cards(
                &project_id,
                ProjectSkillReportOptions {
                    min_linked_outcomes: 1,
                    stale_after_days: 30,
                    now,
                },
            )
            .await
            .unwrap();

        let unused = cards
            .iter()
            .find(|card| card.title == "Unused approved skill")
            .unwrap();
        assert_eq!(unused.aging_status, ProjectSkillAgingStatus::Unused);
        let pinned = cards
            .iter()
            .find(|card| card.title == "Pinned approved skill")
            .unwrap();
        assert_eq!(pinned.aging_status, ProjectSkillAgingStatus::Active);
    }

    #[tokio::test]
    async fn project_skill_service_rejects_pinning_unapproved_skills() {
        let service = ProjectSkillService::new(Arc::new(MemoryProjectSkillRepository::new()));
        let project_id = ProjectId::from_string("project-1".to_string());
        let staged = service.stage_skill(staged_skill(project_id)).await.unwrap();

        let result = service.pin_skill(&staged.id).await;

        assert!(matches!(result, Err(crate::error::AppError::Validation(_))));
    }

    #[tokio::test]
    async fn project_skill_distillation_requires_eligible_outcome() {
        let service = ProjectSkillService::new(Arc::new(MemoryProjectSkillRepository::new()));
        let project_id = ProjectId::from_string("project-1".to_string());
        let outcome = new_empty_task_outcome(project_id, "review", "review_note", "review-1");

        let result = service
            .stage_skill_from_outcome(StageProjectSkillFromOutcomeInput {
                outcome,
                title: "Use review feedback as regression guidance".to_string(),
                bucket: "review".to_string(),
                stage: "review".to_string(),
                scope_paths: Vec::new(),
                compact_guidance: "Check the same regression before approving.".to_string(),
                body_markdown: "Detailed guidance".to_string(),
                predicted_effect: "Reduces repeat review changes.".to_string(),
                additional_provenance: json!({ "test": true }),
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn project_skill_distillation_stages_skill_with_outcome_provenance() {
        let service = ProjectSkillService::new(Arc::new(MemoryProjectSkillRepository::new()));
        let project_id = ProjectId::from_string("project-1".to_string());
        let mut outcome =
            new_empty_task_outcome(project_id.clone(), "merge_validation", "task", "task-1");
        outcome.status = TaskOutcomeStatus::Eligible;
        outcome.outcome_class = Some("merge_validation_failed".to_string());
        outcome.task_id = Some("task-1".to_string());

        let staged = service
            .stage_skill_from_outcome(StageProjectSkillFromOutcomeInput {
                outcome: outcome.clone(),
                title: "Run merge validation before marking complete".to_string(),
                bucket: "merge".to_string(),
                stage: "review".to_string(),
                scope_paths: vec!["src-tauri".to_string()],
                compact_guidance:
                    "Before approving merge recovery, check the validation failure class."
                        .to_string(),
                body_markdown:
                    "Use the validation log as evidence before repeating a failed merge."
                        .to_string(),
                predicted_effect: "Prevents repeated failed merge validation loops.".to_string(),
                additional_provenance: json!({ "distiller": "service-test" }),
            })
            .await
            .unwrap();

        assert_eq!(staged.project_id, project_id);
        assert_eq!(staged.status, ProjectSkillLifecycleStatus::Staged);
        assert_eq!(
            staged.predicted_effect.as_deref(),
            Some("Prevents repeated failed merge validation loops.")
        );
        assert_eq!(
            staged.provenance_json["outcome_id"].as_str(),
            Some(outcome.id.as_str())
        );
        assert_eq!(
            staged.provenance_json["outcome_source"].as_str(),
            Some("merge_validation")
        );
        assert_eq!(
            staged.provenance_json["additional"]["distiller"].as_str(),
            Some("service-test")
        );
    }

    #[tokio::test]
    async fn project_skill_distiller_stages_eligible_outcomes() {
        let outcome_repo = Arc::new(MemoryTaskOutcomeRepository::new());
        let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
        let project_id = ProjectId::from_string("project-1".to_string());
        let mut outcome = new_empty_task_outcome(
            project_id.clone(),
            "github_pr_review",
            "github_review",
            "r1",
        );
        outcome.status = TaskOutcomeStatus::Eligible;
        outcome.outcome_class = Some("github_pr_changes_requested".to_string());
        outcome.evidence_json = json!({
            "scope_paths": ["src-tauri"],
            "comment_count": 2,
        });
        outcome_repo
            .upsert(crate::domain::repositories::UpsertTaskOutcomeInput {
                outcome: outcome.clone(),
            })
            .await
            .unwrap();

        let distiller = ProjectSkillDistillerService::new(outcome_repo, skill_repo);
        let result = distiller
            .distill_eligible_outcomes(DistillEligibleOutcomesInput {
                project_id,
                source: None,
                limit: 10,
                origin: ProjectSkillDistillationOrigin::ManualCurator,
            })
            .await
            .unwrap();

        assert_eq!(result.staged_skills.len(), 1);
        assert_eq!(result.skipped_existing, 0);
        let staged = &result.staged_skills[0];
        assert_eq!(staged.status, ProjectSkillLifecycleStatus::Staged);
        assert_eq!(staged.bucket, "review");
        assert_eq!(staged.stage, "review");
        assert_eq!(staged.scope_paths, vec!["src-tauri".to_string()]);
        assert_eq!(
            staged.provenance_json["outcome_id"].as_str(),
            Some(outcome.id.as_str())
        );
        assert_eq!(
            staged.provenance_json["additional"]["distillation_origin"].as_str(),
            Some("manual_curator")
        );
        assert!(staged
            .predicted_effect
            .as_deref()
            .unwrap_or_default()
            .contains("github pr changes requested"));
    }

    #[tokio::test]
    async fn project_skill_distiller_skips_existing_outcome_provenance() {
        let outcome_repo = Arc::new(MemoryTaskOutcomeRepository::new());
        let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
        let skill_service =
            ProjectSkillService::new(Arc::clone(&skill_repo) as Arc<dyn ProjectSkillRepository>);
        let project_id = ProjectId::from_string("project-1".to_string());
        let mut outcome =
            new_empty_task_outcome(project_id.clone(), "merge_validation", "task", "task-1");
        outcome.status = TaskOutcomeStatus::Eligible;
        outcome.outcome_class = Some("merge_validation_failed".to_string());
        outcome_repo
            .upsert(crate::domain::repositories::UpsertTaskOutcomeInput {
                outcome: outcome.clone(),
            })
            .await
            .unwrap();
        skill_service
            .stage_skill_from_outcome(StageProjectSkillFromOutcomeInput {
                outcome: outcome.clone(),
                title: "Existing candidate".to_string(),
                bucket: "merge".to_string(),
                stage: "merge".to_string(),
                scope_paths: Vec::new(),
                compact_guidance: "Existing guidance".to_string(),
                body_markdown: "Existing body".to_string(),
                predicted_effect: "Existing effect".to_string(),
                additional_provenance: json!({ "test": true }),
            })
            .await
            .unwrap();

        let distiller = ProjectSkillDistillerService::new(outcome_repo, skill_repo);
        let result = distiller
            .distill_eligible_outcomes(DistillEligibleOutcomesInput {
                project_id,
                source: None,
                limit: 10,
                origin: ProjectSkillDistillationOrigin::ManualCurator,
            })
            .await
            .unwrap();

        assert!(result.staged_skills.is_empty());
        assert_eq!(result.skipped_existing, 1);
    }

    #[tokio::test]
    async fn project_skill_distiller_stages_single_eligible_outcome_once() {
        let outcome_repo = Arc::new(MemoryTaskOutcomeRepository::new());
        let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
        let project_id = ProjectId::from_string("project-1".to_string());
        let mut outcome = new_empty_task_outcome(
            project_id.clone(),
            "verification",
            "gap_recurrence",
            "gap-1",
        );
        outcome.status = TaskOutcomeStatus::Eligible;
        outcome.outcome_class = Some("verification_gap_recurring".to_string());

        let distiller = ProjectSkillDistillerService::new(outcome_repo, skill_repo);
        let staged = distiller
            .stage_eligible_outcome_candidate(&outcome)
            .await
            .unwrap()
            .expect("eligible outcome should stage a candidate");

        assert_eq!(staged.project_id, project_id);
        assert_eq!(staged.status, ProjectSkillLifecycleStatus::Staged);
        assert_eq!(
            staged.provenance_json["outcome_id"].as_str(),
            Some(outcome.id.as_str())
        );

        let duplicate = distiller
            .stage_eligible_outcome_candidate(&outcome)
            .await
            .unwrap();
        assert!(duplicate.is_none());
    }

    #[tokio::test]
    async fn project_skill_distiller_records_memory_pipeline_role_origin() {
        let outcome_repo = Arc::new(MemoryTaskOutcomeRepository::new());
        let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
        let project_id = ProjectId::from_string("project-1".to_string());
        let mut outcome =
            new_empty_task_outcome(project_id.clone(), "task_pipeline", "task", "task-1");
        outcome.status = TaskOutcomeStatus::Eligible;
        outcome.outcome_class = Some("repeated_task_failure".to_string());

        let distiller = ProjectSkillDistillerService::new(outcome_repo, skill_repo);
        let staged = distiller
            .stage_eligible_outcome_candidate_with_origin(
                &outcome,
                ProjectSkillDistillationOrigin::MemoryPipelineRole,
            )
            .await
            .unwrap()
            .expect("eligible outcome should stage a candidate");

        assert_eq!(staged.project_id, project_id);
        assert_eq!(
            staged.provenance_json["additional"]["distillation_origin"].as_str(),
            Some("memory_pipeline_role")
        );
        assert_eq!(
            staged.provenance_json["additional"]["pipeline_role"].as_str(),
            Some("memory_capture.skill_distiller")
        );
    }

    #[tokio::test]
    async fn prompt_selected_citations_resolve_only_approved_same_project_skills() {
        let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
        let skill_service = ProjectSkillService::new(skill_repo);
        let project_id = ProjectId::from_string("project-1".to_string());
        let other_project_id = ProjectId::from_string("project-2".to_string());

        let approved = skill_service
            .stage_skill(staged_skill(project_id.clone()))
            .await
            .unwrap();
        let approved = skill_service
            .approve_skill(&approved.id)
            .await
            .unwrap()
            .unwrap();

        let staged = skill_service
            .stage_skill(staged_skill(project_id.clone()))
            .await
            .unwrap();

        let archived = skill_service
            .stage_skill(staged_skill(project_id.clone()))
            .await
            .unwrap();
        let archived = skill_service
            .approve_skill(&archived.id)
            .await
            .unwrap()
            .unwrap();
        skill_service
            .archive_skill(&archived.id)
            .await
            .unwrap()
            .unwrap();

        let other_project = skill_service
            .stage_skill(staged_skill(other_project_id))
            .await
            .unwrap();
        let other_project = skill_service
            .approve_skill(&other_project.id)
            .await
            .unwrap()
            .unwrap();

        let prompt = format!(
            "Use these.\n\
             <!-- ralphx_project_skill={} -->\n\
             <!-- ralphx_project_skill={} -->\n\
             <!-- ralphx_project_skill={} -->\n\
             <!-- ralphx_project_skill={} -->\n\
             <!-- ralphx_project_skill=../bad -->\n\
             <!-- ralphx_project_skill={} -->",
            approved.id.as_str(),
            staged.id.as_str(),
            archived.id.as_str(),
            other_project.id.as_str(),
            approved.id.as_str()
        );

        let citations = skill_service
            .prompt_selected_citations(&project_id, &prompt)
            .await
            .unwrap();

        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].skill_id, approved.id.as_str());
        assert_eq!(
            citations[0].predicted_effect,
            "Avoids adapter-only injection."
        );

        let selected_skills = skill_service
            .prompt_selected_skills(&project_id, &prompt)
            .await
            .unwrap();

        assert_eq!(selected_skills.len(), 1);
        assert_eq!(selected_skills[0].id, approved.id);
        assert_eq!(selected_skills[0].stage, "execution");
        assert_eq!(selected_skills[0].bucket, "execution");
    }
}
