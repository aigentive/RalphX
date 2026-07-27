use async_trait::async_trait;

use crate::domain::entities::{ProjectId, ProjectSkillSettings, ProjectSkillSettingsPatch};
use crate::error::AppResult;

#[async_trait]
pub trait ProjectSkillSettingsRepository: Send + Sync {
    async fn get_for_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Option<ProjectSkillSettings>>;

    async fn upsert(&self, settings: ProjectSkillSettings) -> AppResult<ProjectSkillSettings>;

    async fn patch(
        &self,
        project_id: &ProjectId,
        patch: ProjectSkillSettingsPatch,
    ) -> AppResult<ProjectSkillSettings>;
}
