use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::entities::{ProjectId, ProjectMemorySettings};
use crate::domain::repositories::ProjectMemorySettingsRepository;
use crate::error::AppResult;

#[derive(Default)]
pub struct MemoryProjectMemorySettingsRepository {
    rows: RwLock<HashMap<ProjectId, ProjectMemorySettings>>,
}

impl MemoryProjectMemorySettingsRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, settings: ProjectMemorySettings) {
        self.rows
            .write()
            .await
            .insert(settings.project_id.clone(), settings);
    }
}

#[async_trait]
impl ProjectMemorySettingsRepository for MemoryProjectMemorySettingsRepository {
    async fn get_for_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Option<ProjectMemorySettings>> {
        Ok(self.rows.read().await.get(project_id).cloned())
    }
}
