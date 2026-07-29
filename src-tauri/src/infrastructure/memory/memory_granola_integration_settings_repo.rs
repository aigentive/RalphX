use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::integrations::{GranolaIntegrationSettings, GranolaIntegrationSettingsRepository};

pub struct MemoryGranolaIntegrationSettingsRepository {
    settings: Arc<RwLock<GranolaIntegrationSettings>>,
}

impl Default for MemoryGranolaIntegrationSettingsRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryGranolaIntegrationSettingsRepository {
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(GranolaIntegrationSettings::default())),
        }
    }
}

#[async_trait]
impl GranolaIntegrationSettingsRepository for MemoryGranolaIntegrationSettingsRepository {
    async fn get(&self) -> Result<GranolaIntegrationSettings, Box<dyn std::error::Error>> {
        Ok(self.settings.read().await.clone())
    }

    async fn upsert(
        &self,
        settings: &GranolaIntegrationSettings,
    ) -> Result<GranolaIntegrationSettings, Box<dyn std::error::Error>> {
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
    async fn stores_and_replaces_granola_integration_settings() {
        let repo = MemoryGranolaIntegrationSettingsRepository::new();

        let default = repo.get().await.unwrap();
        assert!(!default.enabled);
        assert!(default.token_secret_ref.is_none());
        assert_eq!(
            default.validation_status,
            IntegrationValidationStatus::NotConfigured
        );

        let settings = GranolaIntegrationSettings {
            enabled: true,
            token_secret_ref: Some("integrations/granola/default/api-token".to_string()),
            validation_status: IntegrationValidationStatus::Valid,
            ..Default::default()
        };

        let saved = repo.upsert(&settings).await.unwrap();
        assert!(saved.enabled);

        let stored = repo.get().await.unwrap();
        assert_eq!(
            stored.token_secret_ref.as_deref(),
            Some("integrations/granola/default/api-token")
        );
        assert_eq!(stored.validation_status, IntegrationValidationStatus::Valid);
    }
}
