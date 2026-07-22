use std::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    prepare_new_project_skill, project_skill_content_matches, refresh_project_skill_metadata,
    ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus, ProjectSkillVersion,
    SkillUsageEvent, TaskOutcome, TaskOutcomeId,
};
use crate::domain::repositories::{
    resolve_task_outcome_upsert, ProjectSkillListOptions, ProjectSkillRepository,
    SkillUsageEventRepository, SkillUsageListOptions, TaskOutcomeListOptions,
    TaskOutcomeRepository, UpsertTaskOutcomeInput,
};
use crate::error::AppResult;

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
        source: &str,
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
            .filter(|row| {
                options
                    .source
                    .as_deref()
                    .map_or(true, |source| row.source == source)
            })
            .filter(|row| options.status.map_or(true, |status| row.status == status))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| std::cmp::Reverse(row.updated_at));
        Ok(rows)
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
    async fn create(&self, skill: ProjectSkill) -> AppResult<ProjectSkill> {
        let skill = prepare_new_project_skill(skill);
        let snapshot = ProjectSkillVersion::from_skill(&skill, skill.updated_at);
        let mut state = self.state.write().unwrap();
        if state.rows.iter().any(|row| row.id == skill.id) {
            return Err(crate::error::AppError::Conflict(format!(
                "project skill {} already exists",
                skill.id.as_str()
            )));
        }
        state.rows.push(skill.clone());
        state.versions.push(snapshot);
        Ok(skill)
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

    async fn update_content(&self, skill: ProjectSkill) -> AppResult<Option<ProjectSkill>> {
        let mut state = self.state.write().unwrap();
        let Some(index) = state
            .rows
            .iter_mut()
            .position(|row| row.id.as_str() == skill.id.as_str())
        else {
            return Ok(None);
        };
        let row = &state.rows[index];
        if row.version != skill.version {
            return Err(crate::error::AppError::Conflict(format!(
                "project skill expected version {} but current version is {}",
                skill.version, row.version
            )));
        }
        if project_skill_content_matches(row, &skill) {
            return Ok(Some(row.clone()));
        }
        let row = &mut state.rows[index];
        row.title = skill.title;
        row.bucket = skill.bucket;
        row.stage = skill.stage;
        row.scope_paths = skill.scope_paths;
        row.compact_guidance = skill.compact_guidance;
        row.body_markdown = skill.body_markdown;
        row.predicted_effect = skill.predicted_effect;
        row.provenance_json = skill.provenance_json;
        row.version += 1;
        row.updated_at = Utc::now();
        refresh_project_skill_metadata(row);
        let saved = row.clone();
        state
            .versions
            .push(ProjectSkillVersion::from_skill(&saved, saved.updated_at));
        Ok(Some(saved))
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

#[derive(Default)]
pub struct MemorySkillUsageEventRepository {
    rows: RwLock<Vec<SkillUsageEvent>>,
}

impl MemorySkillUsageEventRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SkillUsageEventRepository for MemorySkillUsageEventRepository {
    async fn record(&self, event: SkillUsageEvent) -> AppResult<SkillUsageEvent> {
        self.rows.write().unwrap().push(event.clone());
        Ok(event)
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
