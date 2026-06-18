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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn defaults_and_persists_settings() {
        let repo = MemoryAtlassianIntegrationSettingsRepository::default();

        assert!(!repo.get().await.unwrap().enabled);

        let mut settings = AtlassianIntegrationSettings::default();
        settings.enabled = true;
        settings.site_url = Some("https://example.atlassian.net".to_string());

        let saved = repo.upsert(&settings).await.unwrap();
        assert!(saved.enabled);
        assert_eq!(
            saved.site_url.as_deref(),
            Some("https://example.atlassian.net")
        );

        let reloaded = repo.get().await.unwrap();
        assert!(reloaded.enabled);
        assert_eq!(
            reloaded.site_url.as_deref(),
            Some("https://example.atlassian.net")
        );
    }
}
