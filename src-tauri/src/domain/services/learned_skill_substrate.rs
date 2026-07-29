use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    MemoryEntry, MemoryEntryId, MemoryStatus, ProjectSkill, ProjectSkillId,
    ProjectSkillLifecycleStatus, SkillUsageEvent, SkillUsageEventId, SkillUsageInjectionKind,
    TaskOutcome, TaskOutcomeId, TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::domain::repositories::{
    MemoryEntryRepository, ProjectSkillListOptions, ProjectSkillMatchedMutation,
    ProjectSkillRepository, ProjectSkillResolutionCommand, ProjectSkillResolutionIdentity,
    ProjectSkillResolutionIdentityKind, ProjectSkillResolutionIntent, ProjectSkillResolutionResult,
    SkillUsageEventRepository, SkillUsageListOptions, TaskOutcomeListOptions,
    TaskOutcomeRepository, UpsertTaskOutcomeInput,
};
use crate::domain::services::learned_skill_adapters::LearnedSkillConstraintCitation;
use crate::domain::services::project_skill_resolution::{
    import_title_resolution_identity, project_skill_resolution_identities,
    ProjectSkillResolutionService,
};
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
        if !outcome.source.is_live() {
            return Err(AppError::Validation(format!(
                "{} is read-only compatibility data and cannot be recorded",
                outcome.source
            )));
        }
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

pub struct UpdateProjectSkillContentInput {
    pub project_id: ProjectId,
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

impl ProjectSkillService {
    pub fn new(repo: Arc<dyn ProjectSkillRepository>) -> Self {
        Self { repo }
    }

    pub async fn stage_skill(&self, skill: ProjectSkill) -> AppResult<ProjectSkill> {
        Ok(self.stage_skill_resolved(skill).await?.skill)
    }

    pub async fn stage_skill_resolved(
        &self,
        mut skill: ProjectSkill,
    ) -> AppResult<ProjectSkillResolutionResult> {
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
        crate::domain::entities::refresh_project_skill_metadata(&mut skill);
        let mut identities = project_skill_resolution_identities(&skill);
        if skill.provenance_json.get("source").and_then(Value::as_str)
            == Some("project_skill_import")
        {
            identities.push(import_title_resolution_identity(
                &skill.title,
                &skill.bucket,
                &skill.stage,
            ));
        }
        ProjectSkillResolutionService::new(Arc::clone(&self.repo))
            .resolve(ProjectSkillResolutionCommand {
                candidate: skill,
                intent: ProjectSkillResolutionIntent::Upsert {
                    identities,
                    matched_mutation: ProjectSkillMatchedMutation::PatchExisting,
                },
                evidence_markdown: None,
                staging_policy: None,
            })
            .await
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

        let now = Utc::now();
        let mut provenance_json = serde_json::json!({});
        if let Some(source_sync_enabled) = input.source_sync_enabled {
            set_project_skill_source_sync_enabled(&mut provenance_json, source_sync_enabled);
        }
        let candidate = ProjectSkill {
            id: ProjectSkillId::new(),
            project_id: input.project_id,
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
            provenance_json,
            companion_of_skill_id: None,
            content_hash: String::new(),
            evidence_hash: String::new(),
            created_by: crate::domain::entities::ProjectSkillCreatedBy::User,
            pipeline_role: None,
            created_at: now,
            updated_at: now,
        };
        match ProjectSkillResolutionService::new(Arc::clone(&self.repo))
            .resolve(ProjectSkillResolutionCommand {
                candidate,
                intent: ProjectSkillResolutionIntent::ExplicitPatch {
                    target_id: input.project_skill_id,
                },
                evidence_markdown: None,
                staging_policy: None,
            })
            .await
        {
            Ok(result) => Ok(Some(result.skill)),
            Err(AppError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn sync_source_candidate(
        &self,
        project_id: ProjectId,
        candidate: ProjectSkillImportCandidate,
    ) -> AppResult<Option<ProjectSkillResolutionResult>> {
        let Some(external_id) = candidate
            .external_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };
        let identity = ProjectSkillResolutionIdentity {
            kind: ProjectSkillResolutionIdentityKind::SourcePath,
            value: external_id.to_string(),
        };
        let skill = project_skill_from_import_candidate(project_id, candidate);
        match ProjectSkillResolutionService::new(Arc::clone(&self.repo))
            .resolve(ProjectSkillResolutionCommand {
                candidate: skill,
                intent: ProjectSkillResolutionIntent::SourceSync { identity },
                evidence_markdown: None,
                staging_policy: None,
            })
            .await
        {
            Ok(result) => Ok(Some(result)),
            Err(AppError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
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
            let skill = project_skill_from_import_candidate(input.project_id.clone(), candidate);
            imported_skills.push(skill_service.stage_skill(skill).await?);
        }

        Ok(ProjectSkillImportApplyResult {
            preview,
            imported_skills,
        })
    }
}

fn project_skill_from_import_candidate(
    project_id: ProjectId,
    candidate: ProjectSkillImportCandidate,
) -> ProjectSkill {
    let now = Utc::now();
    ProjectSkill {
        id: ProjectSkillId::new(),
        project_id,
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
        content_hash: String::new(),
        evidence_hash: String::new(),
        created_by: crate::domain::entities::ProjectSkillCreatedBy::Imported,
        pipeline_role: None,
        created_at: now,
        updated_at: now,
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
            content_hash: String::new(),
            evidence_hash: String::new(),
            created_by: crate::domain::entities::ProjectSkillCreatedBy::User,
            pipeline_role: None,
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
        self.repo.record(event).await
    }

    pub async fn record_usage_batch(
        &self,
        events: Vec<SkillUsageEvent>,
    ) -> AppResult<Vec<SkillUsageEvent>> {
        self.repo.record_batch(events).await
    }

    pub async fn list_project_usage(
        &self,
        project_id: &ProjectId,
        options: SkillUsageListOptions,
    ) -> AppResult<Vec<SkillUsageEvent>> {
        self.repo.list_by_project(project_id, options).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillUsageAttribution {
    ExactRun {
        conversation_id: String,
        agent_run_id: String,
        provider_harness: String,
        stage: Option<String>,
        bucket: Option<String>,
    },
    BoundedConversation {
        conversation_id: String,
        reason: String,
        stage: Option<String>,
        bucket: Option<String>,
    },
    InteractiveStdin {
        conversation_id: String,
        source_turn_id: String,
        stage: Option<String>,
        bucket: Option<String>,
    },
}

pub fn new_c2_skill_usage_event(
    project_id: ProjectId,
    project_skill_id: ProjectSkillId,
    injection_kind: SkillUsageInjectionKind,
    attribution: SkillUsageAttribution,
) -> AppResult<SkillUsageEvent> {
    validate_non_empty("skill usage project_id", project_id.as_str())?;
    validate_non_empty("skill usage project_skill_id", project_skill_id.as_str())?;

    let (conversation_id, agent_run_id, provider_harness, stage, bucket, anchor, metadata_json) =
        match (injection_kind, attribution) {
            (
                SkillUsageInjectionKind::CompactIndex
                | SkillUsageInjectionKind::ComposerDirective
                | SkillUsageInjectionKind::FullLoad,
                SkillUsageAttribution::ExactRun {
                    conversation_id,
                    agent_run_id,
                    provider_harness,
                    stage,
                    bucket,
                },
            ) => {
                validate_non_empty("skill usage conversation_id", &conversation_id)?;
                validate_non_empty("skill usage agent_run_id", &agent_run_id)?;
                validate_non_empty("skill usage provider_harness", &provider_harness)?;
                let anchor = agent_run_id.clone();
                (
                    conversation_id,
                    Some(agent_run_id),
                    Some(provider_harness),
                    stage,
                    bucket,
                    anchor,
                    serde_json::json!({
                        "scoring_eligible": true,
                        "outcome_linkage_eligible": true,
                        "outcome_linkage_policy": "exact_run",
                    }),
                )
            }
            (
                SkillUsageInjectionKind::FullLoad,
                SkillUsageAttribution::BoundedConversation {
                    conversation_id,
                    reason,
                    stage,
                    bucket,
                },
            ) => {
                validate_non_empty("skill usage conversation_id", &conversation_id)?;
                validate_non_empty("skill usage attribution reason", &reason)?;
                let anchor = conversation_id.clone();
                (
                    conversation_id,
                    None,
                    None,
                    stage,
                    bucket,
                    anchor,
                    serde_json::json!({
                        "scoring_eligible": true,
                        "outcome_linkage_eligible": true,
                        "outcome_linkage_policy": "bounded_conversation",
                        "attribution_reason": reason,
                    }),
                )
            }
            (
                SkillUsageInjectionKind::InteractiveStdinUnattributed,
                SkillUsageAttribution::InteractiveStdin {
                    conversation_id,
                    source_turn_id,
                    stage,
                    bucket,
                },
            ) => {
                validate_non_empty("skill usage conversation_id", &conversation_id)?;
                validate_non_empty("skill usage source_turn_id", &source_turn_id)?;
                let anchor = source_turn_id.clone();
                (
                    conversation_id,
                    None,
                    None,
                    stage,
                    bucket,
                    anchor,
                    serde_json::json!({
                        "scoring_eligible": false,
                        "outcome_linkage_eligible": false,
                        "outcome_linkage_policy": "none",
                        "exclusion_reason": "interactive_stdin_has_no_exact_agent_run",
                        "source_turn_id": source_turn_id,
                    }),
                )
            }
            _ => {
                return Err(AppError::Validation(format!(
                    "invalid attribution policy for {} skill usage",
                    injection_kind.as_str()
                )))
            }
        };

    let id =
        deterministic_skill_usage_event_id(&project_id, &project_skill_id, injection_kind, &anchor);
    Ok(SkillUsageEvent {
        id,
        project_id,
        project_skill_id,
        conversation_id: Some(conversation_id),
        agent_run_id,
        provider_harness,
        stage,
        bucket,
        injection_kind,
        outcome_id: None,
        metadata_json,
        created_at: Utc::now(),
    })
}

fn deterministic_skill_usage_event_id(
    project_id: &ProjectId,
    project_skill_id: &ProjectSkillId,
    injection_kind: SkillUsageInjectionKind,
    anchor: &str,
) -> SkillUsageEventId {
    let mut hasher = Sha256::new();
    for component in [
        "ralphx-skill-usage-v1",
        project_id.as_str(),
        project_skill_id.as_str(),
        injection_kind.as_str(),
        anchor,
    ] {
        hasher.update((component.len() as u64).to_be_bytes());
        hasher.update(component.as_bytes());
    }
    SkillUsageEventId::from_string(format!("skill-usage-{:x}", hasher.finalize()))
}

pub fn new_empty_task_outcome(
    project_id: ProjectId,
    source: TaskOutcomeSource,
    source_ref_kind: impl Into<String>,
    source_ref_id: impl Into<String>,
) -> TaskOutcome {
    let now = Utc::now();
    TaskOutcome {
        id: TaskOutcomeId::new(),
        project_id,
        source,
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
        failure_fingerprint: None,
        provider_harness: None,
        provider_session_id: None,
        created_at: now,
        updated_at: now,
    }
}

pub fn new_skill_usage_event(
    project_id: ProjectId,
    project_skill_id: ProjectSkillId,
    injection_kind: SkillUsageInjectionKind,
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
        injection_kind,
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
