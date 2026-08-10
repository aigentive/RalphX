use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::entities::{ProjectId, ProjectRepositoryCapability};
use crate::domain::repositories::ProjectRepositoryCapabilityRepository;
use crate::error::AppResult;

#[derive(Default)]
pub struct MemoryProjectRepositoryCapabilityRepository {
    rows: RwLock<HashMap<String, ProjectRepositoryCapability>>,
}

impl MemoryProjectRepositoryCapabilityRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ProjectRepositoryCapabilityRepository for MemoryProjectRepositoryCapabilityRepository {
    async fn get(&self, project_id: &ProjectId) -> AppResult<Option<ProjectRepositoryCapability>> {
        Ok(self.rows.read().await.get(project_id.as_str()).cloned())
    }

    async fn upsert(&self, capability: &ProjectRepositoryCapability) -> AppResult<()> {
        self.rows.write().await.insert(
            capability.project_id.as_str().to_string(),
            capability.clone(),
        );
        Ok(())
    }
}
