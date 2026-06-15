use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::entities::{ProjectId, ProjectSkillSettings};
use crate::domain::repositories::ProjectSkillSettingsRepository;
use crate::error::AppResult;

#[derive(Default)]
pub struct MemoryProjectSkillSettingsRepository {
    rows: RwLock<HashMap<ProjectId, ProjectSkillSettings>>,
}

impl MemoryProjectSkillSettingsRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ProjectSkillSettingsRepository for MemoryProjectSkillSettingsRepository {
    async fn get_for_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Option<ProjectSkillSettings>> {
        Ok(self.rows.read().await.get(project_id).cloned())
    }

    async fn upsert(&self, settings: ProjectSkillSettings) -> AppResult<ProjectSkillSettings> {
        self.rows
            .write()
            .await
            .insert(settings.project_id.clone(), settings.clone());
        Ok(settings)
    }
}
