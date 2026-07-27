use async_trait::async_trait;

use crate::domain::entities::{ProjectId, ProjectMemorySettings};
use crate::error::AppResult;

#[async_trait]
pub trait ProjectMemorySettingsRepository: Send + Sync {
    async fn get_for_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Option<ProjectMemorySettings>>;
}
