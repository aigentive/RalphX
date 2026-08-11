use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::integrations::{LinearIntegrationSettings, LinearIntegrationSettingsRepository};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::integrations::IntegrationValidationStatus;

    #[tokio::test]
    async fn stores_and_replaces_linear_integration_settings() {
        let repo = MemoryLinearIntegrationSettingsRepository::new();

        let default = repo.get().await.unwrap();
        assert!(!default.enabled);
        assert_eq!(
            default.validation_status,
            IntegrationValidationStatus::NotConfigured
        );

        let settings = LinearIntegrationSettings {
            enabled: true,
            token_secret_ref: Some("linear-token-ref".to_string()),
            validation_status: IntegrationValidationStatus::Valid,
            issue_search_available: true,
            ..Default::default()
        };

        let saved = repo.upsert(&settings).await.unwrap();
        assert!(saved.enabled);

        let stored = repo.get().await.unwrap();
        assert_eq!(stored.token_secret_ref.as_deref(), Some("linear-token-ref"));
        assert!(stored.issue_search_available);
    }
}
