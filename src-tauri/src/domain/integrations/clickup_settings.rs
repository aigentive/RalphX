use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::IntegrationValidationStatus;

/// Singleton ClickUp ticketing-provider integration settings.
///
/// Secrets are never stored here — only `token_secret_ref` (a keychain reference);
/// the real Personal API token lives in the OS keychain via `SecretStore`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickUpIntegrationSettings {
    pub enabled: bool,
    pub token_secret_ref: Option<String>,
    pub workspace_id: Option<String>,
    pub validation_status: IntegrationValidationStatus,
    pub task_search_available: bool,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl Default for ClickUpIntegrationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            token_secret_ref: None,
            workspace_id: None,
            validation_status: IntegrationValidationStatus::NotConfigured,
            task_search_available: false,
            last_validated_at: None,
            last_error: None,
            updated_at: Utc::now(),
        }
    }
}

#[async_trait]
pub trait ClickUpIntegrationSettingsRepository: Send + Sync {
    async fn get(&self) -> Result<ClickUpIntegrationSettings, Box<dyn std::error::Error>>;

    async fn upsert(
        &self,
        settings: &ClickUpIntegrationSettings,
    ) -> Result<ClickUpIntegrationSettings, Box<dyn std::error::Error>>;
}
