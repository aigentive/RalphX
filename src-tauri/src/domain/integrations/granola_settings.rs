use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::IntegrationValidationStatus;

/// Singleton Granola integration settings.
///
/// Secrets are never stored here — only `token_secret_ref` (a keychain reference);
/// the real Granola API token lives in the OS keychain via `SecretStore`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GranolaIntegrationSettings {
    pub enabled: bool,
    pub token_secret_ref: Option<String>,
    pub validation_status: IntegrationValidationStatus,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl Default for GranolaIntegrationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            token_secret_ref: None,
            validation_status: IntegrationValidationStatus::NotConfigured,
            last_validated_at: None,
            last_error: None,
            updated_at: Utc::now(),
        }
    }
}

#[async_trait]
pub trait GranolaIntegrationSettingsRepository: Send + Sync {
    async fn get(&self) -> Result<GranolaIntegrationSettings, Box<dyn std::error::Error>>;

    async fn upsert(
        &self,
        settings: &GranolaIntegrationSettings,
    ) -> Result<GranolaIntegrationSettings, Box<dyn std::error::Error>>;
}
