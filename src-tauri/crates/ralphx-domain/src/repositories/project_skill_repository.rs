use async_trait::async_trait;

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus};
use crate::error::AppResult;

#[derive(Debug, Clone, Default)]
pub struct ProjectSkillListOptions {
    pub status: Option<ProjectSkillLifecycleStatus>,
    pub include_archived: bool,
    pub stage: Option<String>,
    pub bucket: Option<String>,
    pub scope_path: Option<String>,
}

#[async_trait]
pub trait ProjectSkillRepository: Send + Sync {
    async fn create(&self, skill: ProjectSkill) -> AppResult<ProjectSkill>;

    async fn get_by_id(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>>;

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
        options: ProjectSkillListOptions,
    ) -> AppResult<Vec<ProjectSkill>>;

    async fn update_lifecycle_status(
        &self,
        id: &ProjectSkillId,
        status: ProjectSkillLifecycleStatus,
    ) -> AppResult<Option<ProjectSkill>>;
}
