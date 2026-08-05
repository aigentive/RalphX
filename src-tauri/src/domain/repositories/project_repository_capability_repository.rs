use async_trait::async_trait;

use crate::domain::entities::{ProjectId, ProjectRepositoryCapability};
use crate::error::AppResult;

#[async_trait]
pub trait ProjectRepositoryCapabilityRepository: Send + Sync {
    async fn get(&self, project_id: &ProjectId) -> AppResult<Option<ProjectRepositoryCapability>>;
    async fn upsert(&self, capability: &ProjectRepositoryCapability) -> AppResult<()>;
}
