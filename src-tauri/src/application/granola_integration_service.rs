use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::integrations::{
    GranolaIntegrationSettings, GranolaIntegrationSettingsRepository, IntegrationValidationStatus,
};
use crate::domain::services::SecretStore;

/// Fixed keychain reference for the single Granola Personal API token.
///
/// Granola is a singleton integration, so the reference is stable (no per-save
/// UUID suffix). The raw token lives in the OS keychain via `SecretStore`; only
/// this reference is ever persisted in the settings row.
const GRANOLA_API_TOKEN_SECRET_REF: &str = "integrations/granola/default/api-token";

/// Auth context for Granola REST calls.
///
/// Granola's public API uses an HTTP bearer API key sent in the `Authorization`
/// header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GranolaAuthContext {
    pub api_token: String,
}

/// Boundary to the Granola public API.
///
/// This task only needs credential validation; note/folder listing, detail
/// fetches, pagination, and rate limiting are added by the follow-up Granola
/// API-client task, which extends this trait and supplies `HyperGranolaApiClient`.
#[async_trait]
pub trait GranolaApiClient: Send + Sync {
    /// Low-cost credential check. The production client issues a minimal request
    /// (for example `GET /v1/notes?page_size=1`) and never fetches transcripts.
    async fn validate(&self, auth: &GranolaAuthContext) -> Result<(), String>;
}

/// No-op client used in tests and when the integration is disabled. Validation
/// succeeds so happy-path flows can be exercised without a network.
pub struct EmptyGranolaApiClient;

/// Client used when the Granola HTTP client could not be initialized (for
/// example TLS unavailable, or the real client has not been wired yet); every
/// call fails with the captured reason.
pub struct UnavailableGranolaApiClient {
    reason: String,
}

impl UnavailableGranolaApiClient {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

#[async_trait]
impl GranolaApiClient for EmptyGranolaApiClient {
    async fn validate(&self, _auth: &GranolaAuthContext) -> Result<(), String> {
        Ok(())
    }
}

#[async_trait]
impl GranolaApiClient for UnavailableGranolaApiClient {
    async fn validate(&self, _auth: &GranolaAuthContext) -> Result<(), String> {
        Err(self.reason.clone())
    }
}

pub struct GranolaIntegrationService {
    settings_repo: Arc<dyn GranolaIntegrationSettingsRepository>,
    secret_store: Arc<dyn SecretStore>,
    client: Arc<dyn GranolaApiClient>,
}

impl GranolaIntegrationService {
    pub fn new(
        settings_repo: Arc<dyn GranolaIntegrationSettingsRepository>,
        secret_store: Arc<dyn SecretStore>,
        client: Arc<dyn GranolaApiClient>,
    ) -> Self {
        Self {
            settings_repo,
            secret_store,
            client,
        }
    }

    pub async fn get_settings(&self) -> Result<GranolaIntegrationSettings, String> {
        self.settings_repo
            .get()
            .await
            .map_err(|error| error.to_string())
    }

    /// Persists Granola settings. The token argument is tri-state: `None` leaves
    /// the existing token untouched, `Some("")` clears it (deleting the secret
    /// and returning a not-configured state), and `Some(value)` stores it in the
    /// keychain. Token changes return the integration to a pending, not-enabled
    /// state so the caller re-validates afterwards.
    pub async fn save_settings(
        &self,
        api_token: Option<String>,
    ) -> Result<GranolaIntegrationSettings, String> {
        let mut settings = self.get_settings().await?;
        let mut token_changed = false;
        if let Some(token) = api_token.map(|value| value.trim().to_string()) {
            token_changed = true;
            if token.is_empty() {
                self.clear_token(&mut settings).await?;
            } else {
                self.store_token(&mut settings, &token).await?;
            }
        }
        if token_changed {
            settings.enabled = false;
            settings.validation_status = pending_status_for_settings(&settings);
            settings.last_validated_at = None;
            settings.last_error = None;
        }
        settings.updated_at = chrono::Utc::now();
        self.settings_repo
            .upsert(&settings)
            .await
            .map_err(|error| error.to_string())
    }

    /// Validates the stored token against Granola and enables the integration on
    /// success, recording the resulting validation status either way.
    pub async fn validate_and_enable(&self) -> Result<GranolaIntegrationSettings, String> {
        let mut settings = self.get_settings().await?;
        let auth = self.auth_context(&settings).await?;
        match self.client.validate(&auth).await {
            Ok(()) => {
                settings.enabled = true;
                settings.validation_status = IntegrationValidationStatus::Valid;
                settings.last_error = None;
            }
            Err(error) => {
                settings.enabled = false;
                settings.validation_status = IntegrationValidationStatus::Invalid;
                settings.last_error = Some(error);
            }
        }
        settings.last_validated_at = Some(chrono::Utc::now());
        settings.updated_at = chrono::Utc::now();
        self.settings_repo
            .upsert(&settings)
            .await
            .map_err(|error| error.to_string())
    }

    async fn clear_token(
        &self,
        settings: &mut GranolaIntegrationSettings,
    ) -> Result<(), String> {
        if let Some(secret_ref) = settings.token_secret_ref.as_ref() {
            self.secret_store
                .delete_secret(secret_ref)
                .await
                .map_err(|error| error.to_string())?;
        }
        settings.token_secret_ref = None;
        Ok(())
    }

    async fn store_token(
        &self,
        settings: &mut GranolaIntegrationSettings,
        token: &str,
    ) -> Result<(), String> {
        self.secret_store
            .put_secret(GRANOLA_API_TOKEN_SECRET_REF, token)
            .await
            .map_err(|error| error.to_string())?;
        let stored_token = self
            .secret_store
            .get_secret(GRANOLA_API_TOKEN_SECRET_REF)
            .await
            .map_err(|error| {
                format!(
                    "Granola API token was saved but could not be read back from secure storage: {error}"
                )
            })?
            .ok_or_else(|| {
                "Granola API token was saved but secure storage returned no value".to_string()
            })?;
        if stored_token != token {
            let _ = self
                .secret_store
                .delete_secret(GRANOLA_API_TOKEN_SECRET_REF)
                .await;
            return Err(
                "Granola API token was saved but secure storage returned a different value"
                    .to_string(),
            );
        }
        settings.token_secret_ref = Some(GRANOLA_API_TOKEN_SECRET_REF.to_string());
        Ok(())
    }

    async fn auth_context(
        &self,
        settings: &GranolaIntegrationSettings,
    ) -> Result<GranolaAuthContext, String> {
        let secret_ref = settings
            .token_secret_ref
            .as_deref()
            .ok_or_else(|| "Granola API token is required".to_string())?;
        let api_token = self
            .secret_store
            .get_secret(secret_ref)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Granola API token is missing from secure storage".to_string())?;
        Ok(GranolaAuthContext { api_token })
    }
}

fn pending_status_for_settings(
    settings: &GranolaIntegrationSettings,
) -> IntegrationValidationStatus {
    if settings.token_secret_ref.is_some() {
        IntegrationValidationStatus::Pending
    } else {
        IntegrationValidationStatus::NotConfigured
    }
}
