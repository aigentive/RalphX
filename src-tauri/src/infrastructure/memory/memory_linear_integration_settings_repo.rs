use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::application::{LinearIntegrationSettings, LinearIntegrationSettingsRepository};

pub struct MemoryLinearIntegrationSettingsRepository {
    settings: Arc<RwLock<LinearIntegrationSettings>>,
}

impl Default for MemoryLinearIntegrationSettingsRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryLinearIntegrationSettingsRepository {
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(LinearIntegrationSettings::default())),
        }
    }
}

#[async_trait]
impl LinearIntegrationSettingsRepository for MemoryLinearIntegrationSettingsRepository {
    async fn get(&self) -> Result<LinearIntegrationSettings, Box<dyn std::error::Error>> {
        Ok(self.settings.read().await.clone())
    }

    async fn upsert(
        &self,
        settings: &LinearIntegrationSettings,
    ) -> Result<LinearIntegrationSettings, Box<dyn std::error::Error>> {
        let mut current = self.settings.write().await;
        *current = settings.clone();
        Ok(current.clone())
    }
}
