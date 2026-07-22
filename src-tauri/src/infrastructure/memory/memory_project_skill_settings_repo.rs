use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::entities::{ProjectId, ProjectSkillSettings, ProjectSkillSettingsPatch};
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
        settings.validate()?;
        self.rows
            .write()
            .await
            .insert(settings.project_id.clone(), settings.clone());
        Ok(settings)
    }

    async fn patch(
        &self,
        project_id: &ProjectId,
        patch: ProjectSkillSettingsPatch,
    ) -> AppResult<ProjectSkillSettings> {
        patch.validate()?;
        let mut rows = self.rows.write().await;
        let mut settings = rows
            .get(project_id)
            .cloned()
            .unwrap_or_else(|| ProjectSkillSettings::default_for_project(project_id.clone()));
        patch.apply_to(&mut settings);
        settings.validate()?;
        rows.insert(project_id.clone(), settings.clone());
        Ok(settings)
    }
}
