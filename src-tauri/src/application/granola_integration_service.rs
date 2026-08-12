use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};

use crate::domain::integrations::{
    GranolaIntegrationSettings, GranolaIntegrationSettingsRepository, IntegrationValidationStatus,
};
use crate::domain::services::SecretStore;

mod prompt_expansion;

// Note records and the outbound client port are domain contracts; re-exported
// here so existing `application::granola_integration_service` importers keep
// resolving.
pub use crate::domain::integrations::granola_notes::{
    is_valid_granola_note_id, GranolaApiClient, GranolaApiError, GranolaAuthContext,
    GranolaNoteDetail, GranolaNoteListPage, GranolaNoteSummary, GranolaTranscriptEntry,
};

/// Fixed keychain reference for the single Granola Personal API token.
///
/// Granola is a singleton integration, so the reference is stable (no per-save
/// UUID suffix). The raw token lives in the OS keychain via `SecretStore`; only
/// this reference is ever persisted in the settings row.
const GRANOLA_API_TOKEN_SECRET_REF: &str = "integrations/granola/default/api-token";

#[async_trait]
pub(crate) trait GranolaRequestLimiter: Send + Sync {
    async fn wait_for_request(&self);
}

pub(crate) struct GranolaRateLimiter {
    sustained_limit: usize,
    sustained_window: Duration,
    burst_limit: usize,
    burst_window: Duration,
    state: Mutex<GranolaRateLimiterState>,
}

#[derive(Default)]
struct GranolaRateLimiterState {
    sustained: VecDeque<Instant>,
    burst: VecDeque<Instant>,
}

impl GranolaRateLimiter {
    pub(crate) fn new() -> Self {
        Self::with_limits(5, Duration::from_secs(1), 25, Duration::from_secs(5))
    }

    fn with_limits(
        sustained_limit: usize,
        sustained_window: Duration,
        burst_limit: usize,
        burst_window: Duration,
    ) -> Self {
        Self {
            sustained_limit,
            sustained_window,
            burst_limit,
            burst_window,
            state: Mutex::new(GranolaRateLimiterState::default()),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_limits_for_tests(
        sustained_limit: usize,
        sustained_window: Duration,
        burst_limit: usize,
        burst_window: Duration,
    ) -> Self {
        Self::with_limits(sustained_limit, sustained_window, burst_limit, burst_window)
    }
}

impl Default for GranolaRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GranolaRequestLimiter for GranolaRateLimiter {
    async fn wait_for_request(&self) {
        loop {
            let wait = {
                let mut state = self.state.lock().await;
                let now = Instant::now();
                prune_window(&mut state.sustained, now, self.sustained_window);
                prune_window(&mut state.burst, now, self.burst_window);

                let sustained_ready_at = next_ready_at(
                    &state.sustained,
                    self.sustained_limit,
                    self.sustained_window,
                );
                let burst_ready_at =
                    next_ready_at(&state.burst, self.burst_limit, self.burst_window);
                if let Some(ready_at) = [sustained_ready_at, burst_ready_at]
                    .into_iter()
                    .flatten()
                    .max()
                {
                    ready_at.saturating_duration_since(now)
                } else {
                    state.sustained.push_back(now);
                    state.burst.push_back(now);
                    return;
                }
            };
            sleep(wait).await;
        }
    }
}

fn prune_window(entries: &mut VecDeque<Instant>, now: Instant, window: Duration) {
    while entries
        .front()
        .is_some_and(|recorded_at| *recorded_at + window <= now)
    {
        entries.pop_front();
    }
}

fn next_ready_at(entries: &VecDeque<Instant>, limit: usize, window: Duration) -> Option<Instant> {
    if limit == 0 || entries.len() < limit {
        None
    } else {
        entries.front().map(|recorded_at| *recorded_at + window)
    }
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

    #[cfg(test)]
    fn is_unavailable_for_tests(&self) -> bool {
        true
    }

    async fn fetch_note_detail(
        &self,
        _auth: &GranolaAuthContext,
        _note_id: &str,
        _include_transcript: bool,
    ) -> Result<GranolaNoteDetail, GranolaApiError> {
        Err(GranolaApiError::ApiError(self.reason.clone()))
    }

    async fn list_notes(
        &self,
        _auth: &GranolaAuthContext,
        _page_size: usize,
        _cursor: Option<&str>,
    ) -> Result<GranolaNoteListPage, GranolaApiError> {
        Err(GranolaApiError::ApiError(self.reason.clone()))
    }
}

pub struct GranolaIntegrationService {
    settings_repo: Arc<dyn GranolaIntegrationSettingsRepository>,
    secret_store: Arc<dyn SecretStore>,
    client: Arc<dyn GranolaApiClient>,
    rate_limiter: Arc<dyn GranolaRequestLimiter>,
}

impl GranolaIntegrationService {
    pub fn new(
        settings_repo: Arc<dyn GranolaIntegrationSettingsRepository>,
        secret_store: Arc<dyn SecretStore>,
        client: Arc<dyn GranolaApiClient>,
    ) -> Self {
        Self::new_with_rate_limiter(
            settings_repo,
            secret_store,
            client,
            Arc::new(GranolaRateLimiter::default()),
        )
    }

    pub(crate) fn new_with_rate_limiter(
        settings_repo: Arc<dyn GranolaIntegrationSettingsRepository>,
        secret_store: Arc<dyn SecretStore>,
        client: Arc<dyn GranolaApiClient>,
        rate_limiter: Arc<dyn GranolaRequestLimiter>,
    ) -> Self {
        Self {
            settings_repo,
            secret_store,
            client,
            rate_limiter,
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
        match self.validate_with_rate_limit(&auth).await {
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

    pub async fn list_notes(
        &self,
        page_size: usize,
        cursor: Option<&str>,
    ) -> Result<GranolaNoteListPage, String> {
        let settings = self.get_settings().await?;
        let auth = self.enabled_auth_context(&settings).await?;
        let page_size = page_size.clamp(1, 30);
        self.rate_limiter.wait_for_request().await;
        self.client
            .list_notes(&auth, page_size, cursor)
            .await
            .map_err(granola_api_error_message)
    }

    pub async fn fetch_note_detail_for_user(
        &self,
        note_id: &str,
        include_transcript: bool,
    ) -> Result<GranolaNoteDetail, String> {
        if !is_valid_granola_note_id(note_id) {
            return Err("Granola note id is invalid".to_string());
        }
        let settings = self.get_settings().await?;
        let auth = self.enabled_auth_context(&settings).await?;
        self.fetch_note_detail_with_rate_limit(&auth, note_id, include_transcript)
            .await
            .map_err(granola_api_error_message)
    }

    async fn clear_token(&self, settings: &mut GranolaIntegrationSettings) -> Result<(), String> {
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

    async fn enabled_auth_context(
        &self,
        settings: &GranolaIntegrationSettings,
    ) -> Result<GranolaAuthContext, String> {
        if settings.token_secret_ref.is_none() {
            return Err("Granola API token is not configured".to_string());
        }
        if !settings.enabled || settings.validation_status != IntegrationValidationStatus::Valid {
            return Err("Granola integration is not enabled".to_string());
        }
        self.auth_context(settings).await
    }

    async fn validate_with_rate_limit(&self, auth: &GranolaAuthContext) -> Result<(), String> {
        self.rate_limiter.wait_for_request().await;
        self.client.validate(auth).await
    }

    async fn fetch_note_detail_with_rate_limit(
        &self,
        auth: &GranolaAuthContext,
        note_id: &str,
        include_transcript: bool,
    ) -> Result<GranolaNoteDetail, GranolaApiError> {
        self.rate_limiter.wait_for_request().await;
        self.client
            .fetch_note_detail(auth, note_id, include_transcript)
            .await
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

fn granola_api_error_message(error: GranolaApiError) -> String {
    match error {
        GranolaApiError::NotFound => "Granola note was not found".to_string(),
        GranolaApiError::RateLimited => "Granola API rate limit was reached".to_string(),
        GranolaApiError::ApiError(message) => message,
    }
}
