use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::integrations::{
    AtlassianIntegrationSettings, AtlassianIntegrationSettingsRepository,
};

pub struct MemoryAtlassianIntegrationSettingsRepository {
    settings: Arc<RwLock<AtlassianIntegrationSettings>>,
}

impl Default for MemoryAtlassianIntegrationSettingsRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryAtlassianIntegrationSettingsRepository {
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(AtlassianIntegrationSettings::default())),
        }
    }
}

#[async_trait]
impl AtlassianIntegrationSettingsRepository for MemoryAtlassianIntegrationSettingsRepository {
    async fn get(&self) -> Result<AtlassianIntegrationSettings, Box<dyn std::error::Error>> {
        Ok(self.settings.read().await.clone())
    }

    async fn upsert(
        &self,
        settings: &AtlassianIntegrationSettings,
    ) -> Result<AtlassianIntegrationSettings, Box<dyn std::error::Error>> {
        let mut current = self.settings.write().await;
        *current = settings.clone();
        Ok(current.clone())
    }
}
