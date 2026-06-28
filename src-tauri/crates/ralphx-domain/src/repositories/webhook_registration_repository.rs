use crate::error::AppResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A registered webhook endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookRegistration {
    pub id: String,
    pub api_key_id: String,
    pub url: String,
    /// JSON array of event type strings, None means all events
    pub event_types: Option<String>,
    /// JSON array of project IDs this webhook covers
    pub project_ids: String,
    /// HMAC-SHA256 secret (64 hex chars)
    pub secret: String,
    pub active: bool,
    pub failure_count: i64,
    pub last_failure_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Repository trait for WebhookRegistration persistence
#[async_trait]
pub trait WebhookRegistrationRepository: Send + Sync {
    /// Create or update (idempotent by url+api_key_id): if url already exists for this api_key_id,
    /// refresh `project_ids` and `event_types` from the incoming registration, reset
    /// `failure_count` to 0, set `active=true`, and return the existing `id`.
    /// Otherwise insert a new row. The `secret` is never regenerated on re-registration.
    async fn upsert(&self, registration: WebhookRegistration) -> AppResult<WebhookRegistration>;

    /// Get by ID
    async fn get_by_id(&self, id: &str) -> AppResult<Option<WebhookRegistration>>;

    /// Get by url + api_key_id (for idempotency check)
    async fn get_by_url_and_key(
        &self,
        url: &str,
        api_key_id: &str,
    ) -> AppResult<Option<WebhookRegistration>>;

    /// List all registrations for an API key
    async fn list_by_api_key(&self, api_key_id: &str) -> AppResult<Vec<WebhookRegistration>>;

    /// Mark as inactive (soft delete)
    async fn deactivate(&self, id: &str, api_key_id: &str) -> AppResult<bool>;

    /// Increment failure count; mark inactive if failure_count >= 10
    async fn increment_failure(&self, id: &str) -> AppResult<()>;

    /// Reset failure count to 0, set active=true
    async fn reset_failures(&self, id: &str) -> AppResult<()>;

    /// List all active webhook registrations for a given project.
    /// Returns webhooks where `active = true` AND the project_ids JSON array
    /// contains the given project_id string.
    async fn list_active_for_project(
        &self,
        project_id: &str,
    ) -> AppResult<Vec<WebhookRegistration>>;
}
