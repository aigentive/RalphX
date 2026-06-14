use std::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus, SkillUsageEvent, TaskOutcome,
    TaskOutcomeId,
};
use crate::domain::repositories::{
    ProjectSkillListOptions, ProjectSkillRepository, SkillUsageEventRepository,
    SkillUsageListOptions, TaskOutcomeListOptions, TaskOutcomeRepository, UpsertTaskOutcomeInput,
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
        if let Some(existing) = rows.iter_mut().find(|row| {
            row.project_id == input.outcome.project_id
                && row.source == input.outcome.source
                && row.source_ref_kind == input.outcome.source_ref_kind
                && row.source_ref_id == input.outcome.source_ref_id
        }) {
            existing.outcome_class = input.outcome.outcome_class;
            existing.status = input.outcome.status;
            existing.evidence_json = input.outcome.evidence_json;
            existing.provider_harness = input.outcome.provider_harness;
            existing.provider_session_id = input.outcome.provider_session_id;
            existing.updated_at = Utc::now();
            return Ok(existing.clone());
        }
        rows.push(input.outcome.clone());
        Ok(input.outcome)
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
pub struct MemoryProjectSkillRepository {
    rows: RwLock<Vec<ProjectSkill>>,
}

impl MemoryProjectSkillRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ProjectSkillRepository for MemoryProjectSkillRepository {
    async fn create(&self, skill: ProjectSkill) -> AppResult<ProjectSkill> {
        self.rows.write().unwrap().push(skill.clone());
        Ok(skill)
    }

    async fn get_by_id(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
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
        options: ProjectSkillListOptions,
    ) -> AppResult<Vec<ProjectSkill>> {
        let mut rows = self
            .rows
            .read()
            .unwrap()
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

    async fn update_lifecycle_status(
        &self,
        id: &ProjectSkillId,
        status: ProjectSkillLifecycleStatus,
    ) -> AppResult<Option<ProjectSkill>> {
        let mut rows = self.rows.write().unwrap();
        let Some(row) = rows.iter_mut().find(|row| row.id.as_str() == id.as_str()) else {
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
