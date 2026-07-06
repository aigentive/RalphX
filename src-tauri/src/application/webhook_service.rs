// Webhook registration service — handles CRUD with business logic
// Secret generation, project scope enforcement, idempotent URL handling

use std::sync::Arc;

use rand::Rng;

use crate::domain::repositories::{WebhookRegistration, WebhookRegistrationRepository};
use crate::error::{AppError, AppResult};

pub struct WebhookService {
    repo: Arc<dyn WebhookRegistrationRepository>,
}

impl WebhookService {
    pub fn new(repo: Arc<dyn WebhookRegistrationRepository>) -> Self {
        Self { repo }
    }

    /// Register a webhook URL. Idempotent: same URL+api_key returns existing registration.
    /// Validates that all requested project_ids are within the key's authorized project_ids.
    /// Generates a cryptographically random HMAC secret on first registration.
    pub async fn register(
        &self,
        api_key_id: &str,
        url: &str,
        event_types: Option<Vec<String>>,
        requested_project_ids: Vec<String>,
        authorized_project_ids: &[String],
    ) -> AppResult<WebhookRegistration> {
        // Project scope enforcement: if authorized_project_ids is non-empty,
        // all requested_project_ids must be within it.
        if !authorized_project_ids.is_empty() {
            for pid in &requested_project_ids {
                if !authorized_project_ids.contains(pid) {
                    return Err(AppError::Validation(format!(
                        "Project '{}' is not in this API key's authorized scope",
                        pid
                    )));
                }
            }
        }

        // Determine effective project_ids:
        // If no requested_project_ids → store '[]' (empty = match all projects)
        // If explicit project_ids provided → use them (already validated against authorized scope above)
        let effective_project_ids = requested_project_ids;

        let project_ids_json = serde_json::to_string(&effective_project_ids)
            .map_err(|e| AppError::Infrastructure(e.to_string()))?;

        let event_types_json = event_types
            .map(|ev| serde_json::to_string(&ev).unwrap_or_default());

        // Generate new id and secret (used only if this is a new registration)
        let new_id = uuid::Uuid::new_v4().to_string();
        let secret = generate_webhook_secret();

        let registration = WebhookRegistration {
            id: new_id,
            api_key_id: api_key_id.to_string(),
            url: url.to_string(),
            event_types: event_types_json,
            project_ids: project_ids_json,
            secret,
            active: true,
            failure_count: 0,
            last_failure_at: None,
            created_at: String::new(), // set by DB
            updated_at: String::new(), // set by DB
        };

        self.repo.upsert(registration).await
    }

    /// Unregister a webhook. Returns true if found and deactivated, false if not found.
    /// Enforces api_key_id ownership.
    pub async fn unregister(&self, webhook_id: &str, api_key_id: &str) -> AppResult<bool> {
        self.repo.deactivate(webhook_id, api_key_id).await
    }

    /// List active webhooks for an API key.
    pub async fn list(&self, api_key_id: &str) -> AppResult<Vec<WebhookRegistration>> {
        let all = self.repo.list_by_api_key(api_key_id).await?;
        Ok(all.into_iter().filter(|r| r.active).collect())
    }
}

/// Generate a cryptographically random 64-character hex secret (32 bytes = 256 bits)
fn generate_webhook_secret() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
    bytes.iter().fold(String::with_capacity(64), |mut acc, b| {
        use std::fmt::Write as _;
        let _ = write!(acc, "{:02x}", b);
        acc
    })
}
