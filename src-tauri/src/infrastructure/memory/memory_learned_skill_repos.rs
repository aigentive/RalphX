use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::entities::learned_skill::{
    is_valid_recurrence_key, task_outcome_recurrence_metadata, TaskOutcomeRecurrenceCorpus,
};
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus, ProjectSkillVersion,
    SkillUsageEvent, TaskOutcome, TaskOutcomeId, TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::domain::repositories::{
    resolve_task_outcome_upsert, ProjectSkillListOptions, ProjectSkillRepository,
    ProjectSkillResolutionCommand, ProjectSkillResolutionOutcome, ProjectSkillResolutionResult,
    SkillUsageEventRepository, SkillUsageListOptions, TaskOutcomeListOptions,
    TaskOutcomeRepository, UpsertTaskOutcomeInput,
};
use crate::domain::services::project_skill_resolution::{
    enforce_project_skill_staging_policy, evaluate_project_skill_resolution,
};
use crate::error::{AppError, AppResult};

#[derive(Default)]
pub struct MemoryTaskOutcomeRepository {
    rows: RwLock<Vec<TaskOutcome>>,
}

impl MemoryTaskOutcomeRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl TaskOutcomeRepository for MemoryTaskOutcomeRepository {
    async fn upsert(&self, input: UpsertTaskOutcomeInput) -> AppResult<TaskOutcome> {
        let mut rows = self.rows.write().unwrap();
        if let Some(existing_index) = rows.iter().position(|row| {
            row.project_id == input.outcome.project_id
                && row.source == input.outcome.source
                && row.source_ref_kind == input.outcome.source_ref_kind
                && row.source_ref_id == input.outcome.source_ref_id
        }) {
            let resolution =
                resolve_task_outcome_upsert(Some(&rows[existing_index]), input.outcome);
            if resolution.should_write {
                let mut outcome = resolution.outcome;
                outcome.updated_at = Utc::now();
                rows[existing_index] = outcome;
            }
            return Ok(rows[existing_index].clone());
        }
        let outcome = resolve_task_outcome_upsert(None, input.outcome).outcome;
        rows.push(outcome.clone());
        Ok(outcome)
    }

    async fn get_by_dedupe(
        &self,
        project_id: &ProjectId,
        source: TaskOutcomeSource,
        source_ref_kind: &str,
        source_ref_id: &str,
    ) -> AppResult<Option<TaskOutcome>> {
        Ok(self
            .rows
            .read()
            .unwrap()
            .iter()
            .find(|row| {
                &row.project_id == project_id
                    && row.source == source
                    && row.source_ref_kind == source_ref_kind
                    && row.source_ref_id == source_ref_id
            })
            .cloned())
    }

    async fn get_by_id(&self, id: &TaskOutcomeId) -> AppResult<Option<TaskOutcome>> {
        Ok(self
            .rows
            .read()
            .unwrap()
            .iter()
            .find(|row| row.id.as_str() == id.as_str())
            .cloned())
    }

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
        options: TaskOutcomeListOptions,
    ) -> AppResult<Vec<TaskOutcome>> {
        let mut rows = self
            .rows
            .read()
            .unwrap()
            .iter()
            .filter(|row| &row.project_id == project_id)
            .filter(|row| options.source.map_or(true, |source| row.source == source))
            .filter(|row| options.status.map_or(true, |status| row.status == status))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| std::cmp::Reverse(row.updated_at));
        Ok(rows)
    }

    async fn recurrence_corpus(
        &self,
        project_id: &ProjectId,
        recurrence_key: &str,
    ) -> AppResult<TaskOutcomeRecurrenceCorpus> {
        if !is_valid_recurrence_key(recurrence_key) {
            return Err(AppError::Validation(
                "task outcome recurrence key is invalid".to_string(),
            ));
        }
        let rows = self.rows.read().unwrap();
        let eligible_sessions = rows
            .iter()
            .filter(|row| &row.project_id == project_id)
            .filter(|row| {
                matches!(
                    row.source,
                    TaskOutcomeSource::Review
                        | TaskOutcomeSource::Merge
                        | TaskOutcomeSource::MergeValidation
                        | TaskOutcomeSource::AgentConversation
                )
            })
            .filter(|row| {
                matches!(
                    row.status,
                    TaskOutcomeStatus::Failed | TaskOutcomeStatus::Eligible
                )
            })
            .filter_map(task_outcome_recurrence_metadata)
            .filter(|(key, _)| *key == recurrence_key)
            .map(|(_, session)| session)
            .collect::<Vec<_>>();
        let distinct_sessions = eligible_sessions
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .len();
        Ok(TaskOutcomeRecurrenceCorpus {
            eligible_observations: eligible_sessions.len() as u64,
            distinct_sessions: distinct_sessions as u64,
        })
    }
}

#[derive(Default)]
struct MemoryProjectSkillState {
    rows: Vec<ProjectSkill>,
    versions: Vec<ProjectSkillVersion>,
}

#[derive(Default)]
pub struct MemoryProjectSkillRepository {
    state: RwLock<MemoryProjectSkillState>,
}

impl MemoryProjectSkillRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ProjectSkillRepository for MemoryProjectSkillRepository {
    async fn resolve(
        &self,
        command: ProjectSkillResolutionCommand,
    ) -> AppResult<ProjectSkillResolutionResult> {
        let mut state = self.state.write().unwrap();
        let candidates = state
            .rows
            .iter()
            .filter(|skill| skill.project_id == command.candidate.project_id)
            .cloned()
            .collect::<Vec<_>>();
        let staging_policy = command.staging_policy.clone();
        let plan = evaluate_project_skill_resolution(command, &candidates)?;
        if plan.outcome == ProjectSkillResolutionOutcome::Duplicate {
            return Ok(ProjectSkillResolutionResult {
                outcome: plan.outcome,
                skill: plan.skill,
                version: None,
            });
        }
        enforce_project_skill_staging_policy(staging_policy.as_ref(), &candidates, &plan)?;
        validate_memory_companion(&state, &plan.skill)?;
        match plan.outcome {
            ProjectSkillResolutionOutcome::CreateNew => {
                if state.rows.iter().any(|row| row.id == plan.skill.id) {
                    return Err(AppError::Conflict(format!(
                        "project skill {} already exists",
                        plan.skill.id.as_str()
                    )));
                }
                state.rows.push(plan.skill.clone());
            }
            ProjectSkillResolutionOutcome::PatchExisting
            | ProjectSkillResolutionOutcome::AppendEvidence => {
                let row = state
                    .rows
                    .iter_mut()
                    .find(|row| row.id == plan.skill.id)
                    .ok_or_else(|| {
                        AppError::Conflict(
                            "project skill resolution target changed concurrently".to_string(),
                        )
                    })?;
                *row = plan.skill.clone();
            }
            ProjectSkillResolutionOutcome::Duplicate => {
                return Err(AppError::Conflict(
                    "duplicate project skill resolution reached the mutation path".to_string(),
                ));
            }
        }
        let next_version = state
            .versions
            .iter()
            .filter(|version| version.project_skill_id == plan.skill.id)
            .map(|version| version.version)
            .max()
            .unwrap_or(0)
            + 1;
        let version = ProjectSkillVersion::from_skill(&plan.skill, next_version, Utc::now());
        version.validate()?;
        state.versions.push(version.clone());
        Ok(ProjectSkillResolutionResult {
            outcome: plan.outcome,
            skill: plan.skill,
            version: Some(version),
        })
    }

    #[cfg(any(test, feature = "test-utils"))]
    async fn seed_for_test(&self, skill: ProjectSkill) -> AppResult<ProjectSkill> {
        let skill = crate::domain::entities::prepare_new_project_skill(skill);
        let mut state = self.state.write().unwrap();
        if state.rows.iter().any(|row| row.id == skill.id) {
            return Err(AppError::Conflict(format!(
                "project skill {} already exists",
                skill.id.as_str()
            )));
        }
        state.rows.push(skill.clone());
        Ok(skill)
    }

    #[cfg(any(test, feature = "test-utils"))]
    async fn create(&self, skill: ProjectSkill) -> AppResult<ProjectSkill> {
        self.seed_for_test(skill).await
    }

    #[cfg(any(test, feature = "test-utils"))]
    async fn update_content(&self, skill: ProjectSkill) -> AppResult<Option<ProjectSkill>> {
        let mut state = self.state.write().unwrap();
        let Some(row) = state.rows.iter_mut().find(|row| row.id == skill.id) else {
            return Ok(None);
        };
        if crate::domain::entities::project_skill_content_matches(row, &skill) {
            return Ok(Some(row.clone()));
        }
        row.title = skill.title;
        row.bucket = skill.bucket;
        row.stage = skill.stage;
        row.scope_paths = skill.scope_paths;
        row.compact_guidance = skill.compact_guidance;
        row.body_markdown = skill.body_markdown;
        row.predicted_effect = skill.predicted_effect;
        row.provenance_json = skill.provenance_json;
        row.updated_at = Utc::now();
        crate::domain::entities::refresh_project_skill_metadata(row);
        Ok(Some(row.clone()))
    }

    #[cfg(any(test, feature = "test-utils"))]
    async fn append_version(&self, version: ProjectSkillVersion) -> AppResult<ProjectSkillVersion> {
        version.validate()?;
        let mut state = self.state.write().unwrap();
        if !state
            .rows
            .iter()
            .any(|row| row.id == version.project_skill_id && row.project_id == version.project_id)
        {
            return Err(AppError::NotFound(format!(
                "project skill {} was not found",
                version.project_skill_id.as_str()
            )));
        }
        if state.versions.iter().any(|row| {
            row.project_skill_id == version.project_skill_id && row.version == version.version
        }) {
            return Err(AppError::Conflict(format!(
                "project skill version {} already exists",
                version.version
            )));
        }
        state.versions.push(version.clone());
        Ok(version)
    }

    async fn get_by_id(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
        Ok(self
            .state
            .read()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.id.as_str() == id.as_str())
            .cloned())
    }

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
        options: ProjectSkillListOptions,
    ) -> AppResult<Vec<ProjectSkill>> {
        let mut rows = self
            .state
            .read()
            .unwrap()
            .rows
            .iter()
            .filter(|row| &row.project_id == project_id)
            .filter(|row| options.include_archived || !row.archived)
            .filter(|row| options.status.map_or(true, |status| row.status == status))
            .filter(|row| {
                options
                    .stage
                    .as_deref()
                    .map_or(true, |stage| row.stage == stage)
            })
            .filter(|row| {
                options
                    .bucket
                    .as_deref()
                    .map_or(true, |bucket| row.bucket == bucket)
            })
            .filter(|row| {
                options.scope_path.as_deref().map_or(true, |scope_path| {
                    row.scope_paths.is_empty()
                        || row
                            .scope_paths
                            .iter()
                            .any(|path| scope_path.starts_with(path))
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
                .then_with(|| a.title.cmp(&b.title))
        });
        Ok(rows)
    }

    async fn list_versions(&self, id: &ProjectSkillId) -> AppResult<Vec<ProjectSkillVersion>> {
        let mut versions = self
            .state
            .read()
            .unwrap()
            .versions
            .iter()
            .filter(|row| row.project_skill_id.as_str() == id.as_str())
            .cloned()
            .collect::<Vec<_>>();
        versions.sort_by_key(|row| row.version);
        Ok(versions)
    }

    async fn update_lifecycle_status(
        &self,
        id: &ProjectSkillId,
        status: ProjectSkillLifecycleStatus,
    ) -> AppResult<Option<ProjectSkill>> {
        let mut state = self.state.write().unwrap();
        let Some(row) = state
            .rows
            .iter_mut()
            .find(|row| row.id.as_str() == id.as_str())
        else {
            return Ok(None);
        };
        row.status = status;
        row.archived = matches!(
            status,
            ProjectSkillLifecycleStatus::Archived | ProjectSkillLifecycleStatus::Retired
        );
        row.updated_at = Utc::now();
        Ok(Some(row.clone()))
    }

    async fn update_pinned(
        &self,
        id: &ProjectSkillId,
        pinned: bool,
    ) -> AppResult<Option<ProjectSkill>> {
        let mut state = self.state.write().unwrap();
        let Some(row) = state
            .rows
            .iter_mut()
            .find(|row| row.id.as_str() == id.as_str())
        else {
            return Ok(None);
        };
        row.pinned = pinned;
        row.updated_at = Utc::now();
        Ok(Some(row.clone()))
    }
}

fn validate_memory_companion(
    state: &MemoryProjectSkillState,
    skill: &ProjectSkill,
) -> AppResult<()> {
    let Some(companion_id) = skill.companion_of_skill_id.as_ref() else {
        return Ok(());
    };
    let companion = state
        .rows
        .iter()
        .find(|row| row.id == *companion_id)
        .ok_or_else(|| {
            AppError::Validation(format!(
                "companion project skill {} was not found",
                companion_id.as_str()
            ))
        })?;
    if companion.project_id != skill.project_id
        || companion.status != ProjectSkillLifecycleStatus::Approved
        || companion.archived
    {
        return Err(AppError::Validation(
            "companion project skill must be an active approved skill in the same project"
                .to_string(),
        ));
    }
    Ok(())
}

#[derive(Default)]
pub struct MemorySkillUsageEventRepository {
    rows: RwLock<Vec<SkillUsageEvent>>,
    fail_next_batch: AtomicBool,
}

impl MemorySkillUsageEventRepository {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn fail_next_batch_for_test(&self) {
        self.fail_next_batch.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl SkillUsageEventRepository for MemorySkillUsageEventRepository {
    async fn record(&self, event: SkillUsageEvent) -> AppResult<SkillUsageEvent> {
        let mut saved = self.record_batch(vec![event]).await?;
        saved.pop().ok_or_else(|| {
            AppError::Database("skill usage batch unexpectedly returned no event".to_string())
        })
    }

    async fn record_batch(&self, events: Vec<SkillUsageEvent>) -> AppResult<Vec<SkillUsageEvent>> {
        if self.fail_next_batch.swap(false, Ordering::SeqCst) {
            return Err(AppError::Database(
                "injected memory skill usage batch failure".to_string(),
            ));
        }

        let mut rows = self.rows.write().unwrap();
        let mut known_ids = rows
            .iter()
            .map(|row| row.id.as_str().to_string())
            .collect::<HashSet<_>>();
        let saved = events;
        for event in &saved {
            if known_ids.insert(event.id.as_str().to_string()) {
                rows.push(event.clone());
            }
        }
        Ok(saved)
    }

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
        options: SkillUsageListOptions,
    ) -> AppResult<Vec<SkillUsageEvent>> {
        let mut rows = self
            .rows
            .read()
            .unwrap()
            .iter()
            .filter(|row| &row.project_id == project_id)
            .filter(|row| {
                options
                    .project_skill_id
                    .as_ref()
                    .map_or(true, |id| row.project_skill_id.as_str() == id.as_str())
            })
            .filter(|row| {
                options
                    .agent_run_id
                    .as_deref()
                    .map_or(true, |agent_run_id| {
                        row.agent_run_id.as_deref() == Some(agent_run_id)
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| std::cmp::Reverse(row.created_at));
        Ok(rows)
    }
}
