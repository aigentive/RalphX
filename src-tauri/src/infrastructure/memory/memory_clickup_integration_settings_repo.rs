use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::integrations::{ClickUpIntegrationSettings, ClickUpIntegrationSettingsRepository};

pub struct MemoryClickUpIntegrationSettingsRepository {
    settings: Arc<RwLock<ClickUpIntegrationSettings>>,
}

impl Default for MemoryClickUpIntegrationSettingsRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryClickUpIntegrationSettingsRepository {
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(ClickUpIntegrationSettings::default())),
        }
    }
}

#[async_trait]
impl ClickUpIntegrationSettingsRepository for MemoryClickUpIntegrationSettingsRepository {
    async fn get(&self) -> Result<ClickUpIntegrationSettings, Box<dyn std::error::Error>> {
        Ok(self.settings.read().await.clone())
    }

    async fn upsert(
        &self,
        settings: &ClickUpIntegrationSettings,
    ) -> Result<ClickUpIntegrationSettings, Box<dyn std::error::Error>> {
        let mut current = self.settings.write().await;
        *current = settings.clone();
        Ok(current.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::integrations::IntegrationValidationStatus;

    #[tokio::test]
    async fn stores_and_replaces_clickup_integration_settings() {
        let repo = MemoryClickUpIntegrationSettingsRepository::new();

        let default = repo.get().await.unwrap();
        assert!(!default.enabled);
        assert!(default.workspace_id.is_none());
        assert_eq!(
            default.validation_status,
            IntegrationValidationStatus::NotConfigured
        );

        let settings = ClickUpIntegrationSettings {
            enabled: true,
            token_secret_ref: Some("clickup-token-ref".to_string()),
            workspace_id: Some("9000123".to_string()),
            validation_status: IntegrationValidationStatus::Valid,
            task_search_available: true,
            ..Default::default()
        };

        let saved = repo.upsert(&settings).await.unwrap();
        assert!(saved.enabled);

        let stored = repo.get().await.unwrap();
        assert_eq!(stored.token_secret_ref.as_deref(), Some("clickup-token-ref"));
        assert_eq!(stored.workspace_id.as_deref(), Some("9000123"));
        assert!(stored.task_search_available);
    }
}
