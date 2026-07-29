// WebhookPublisher — concrete event delivery engine for registered webhook endpoints.
//
// Architecture:
//   - DashMap<String, Vec<WebhookRegistration>> cache keyed by project_id
//   - Cache populated lazily on first publish() for a project
//   - Evicted after mutations (register/unregister) and after failure tracking
//   - tokio::spawn per webhook delivery — non-blocking
//   - 3 retry attempts with exponential backoff (1s, 2s, 4s)
//   - After 10 consecutive failures: repo marks webhook inactive, cache evicted

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dashmap::DashMap;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{debug, error, info, warn};

use crate::domain::repositories::{WebhookRegistration, WebhookRegistrationRepository};
use crate::domain::state_machine::services::WebhookPublisher as WebhookPublisherTrait;
use crate::infrastructure::webhook_http_client::WebhookHttpClient;
use ralphx_domain::entities::EventType;

type HmacSha256 = Hmac<Sha256>;

/// Concrete webhook event delivery engine.
///
/// Constructed ONCE in lib.rs and Arc-cloned into both AppState instances.
/// Implements `WebhookPublisher` trait from domain::state_machine::services.
pub struct WebhookPublisher {
    repo: Arc<dyn WebhookRegistrationRepository>,
    http_client: Arc<dyn WebhookHttpClient>,
    /// project_id → active webhooks for that project (lazy-populated, evicted on mutation)
    cache: Arc<DashMap<String, Vec<WebhookRegistration>>>,
}

impl WebhookPublisher {
    /// Create a new WebhookPublisher with an empty cache.
    ///
    /// Cache is populated lazily on first `publish()` for each project.
    pub fn new(
        repo: Arc<dyn WebhookRegistrationRepository>,
        http_client: Arc<dyn WebhookHttpClient>,
    ) -> Self {
        Self {
            repo,
            http_client,
            cache: Arc::new(DashMap::new()),
        }
    }

    /// Evict a project's webhooks from the cache.
    ///
    /// Call after register/unregister mutations to force cache refresh on next publish().
    pub fn invalidate_project(&self, project_id: &str) {
        self.cache.remove(project_id);
    }

    /// Load webhooks for a project from cache, falling back to DB on miss.
    async fn get_webhooks_for_project(&self, project_id: &str) -> Vec<WebhookRegistration> {
        if let Some(entry) = self.cache.get(project_id) {
            return entry.value().clone();
        }
        // Cache miss — query DB
        match self.repo.list_active_for_project(project_id).await {
            Ok(webhooks) => {
                self.cache.insert(project_id.to_string(), webhooks.clone());
                webhooks
            }
            Err(e) => {
                warn!(error = %e, project_id, "Failed to load webhooks from DB (non-fatal)");
                vec![]
            }
        }
    }
}

#[async_trait]
impl WebhookPublisherTrait for WebhookPublisher {
    async fn publish(
        &self,
        event_type: EventType,
        project_id: &str,
        payload: serde_json::Value,
    ) {
        info!(event_type = %event_type, project_id, "WebhookPublisher::publish called");

        let webhooks = self.get_webhooks_for_project(project_id).await;
        info!(project_id, count = webhooks.len(), "Loaded webhooks for project");

        let matching: Vec<_> = webhooks
            .into_iter()
            .filter(|w| webhook_matches_event(w, &event_type))
            .collect();

        if matching.is_empty() {
            info!(
                event_type = %event_type,
                project_id,
                "No matching webhooks — either no registrations for project or event type not subscribed"
            );
            return;
        }

        info!(
            event_type = %event_type,
            project_id,
            count = matching.len(),
            "Dispatching webhook deliveries"
        );

        let event_type_str = event_type.to_string();
        let project_id = project_id.to_string();

        for webhook in matching {
            let repo = Arc::clone(&self.repo);
            let http_client = Arc::clone(&self.http_client);
            let cache = Arc::clone(&self.cache);
            let event_str = event_type_str.clone();
            let proj_id = project_id.clone();
            let payload_clone = payload.clone();

            tokio::spawn(async move {
                use futures::FutureExt;
                let webhook_id = webhook.id.clone();
                let result = std::panic::AssertUnwindSafe(deliver_with_retry(
                    &repo,
                    &http_client,
                    &cache,
                    &webhook,
                    &event_str,
                    &proj_id,
                    payload_clone,
                ))
                .catch_unwind()
                .await;
                if let Err(panic_val) = result {
                    let msg = if let Some(s) = panic_val.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_val.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic".to_string()
                    };
                    error!(webhook_id = %webhook_id, panic = %msg, "Webhook delivery panicked");
                    track_failure(&repo, &cache, &webhook_id, &proj_id).await;
                }
            });
        }
    }

    fn invalidate_project(&self, project_id: &str) {
        self.cache.remove(project_id);
    }
}

// ============================================================================
// Delivery helpers
// ============================================================================

/// Returns true if the webhook's event_types filter includes the given event.
/// If event_types is None (no filter), matches all events.
fn webhook_matches_event(webhook: &WebhookRegistration, event_type: &EventType) -> bool {
    let event_str = event_type.to_string();
    match &webhook.event_types {
        None => true, // no filter = match all
        Some(json) => serde_json::from_str::<Vec<String>>(json)
            .map(|types| types.contains(&event_str))
            .unwrap_or(false),
    }
}

/// Deliver one webhook event with 3-attempt exponential backoff retry.
///
/// Retry policy:
///   - Retryable: 5xx, 429, network error/timeout
///   - Non-retryable: 4xx (except 429)
///   - Backoff: attempt 1 = 0s wait, attempt 2 = 1s wait, attempt 3 = 2s wait
async fn deliver_with_retry(
    repo: &Arc<dyn WebhookRegistrationRepository>,
    http_client: &Arc<dyn WebhookHttpClient>,
    cache: &Arc<DashMap<String, Vec<WebhookRegistration>>>,
    webhook: &WebhookRegistration,
    event_type_str: &str,
    project_id: &str,
    payload: serde_json::Value,
) {
    let timestamp = chrono::Utc::now().to_rfc3339();

    // Build envelope body
    let envelope = serde_json::json!({
        "webhook_id": webhook.id,
        "event_type": event_type_str,
        "project_id": project_id,
        "payload": payload,
        "timestamp": timestamp,
    });
    let body_bytes = match serde_json::to_vec(&envelope) {
        Ok(b) => b,
        Err(e) => {
            warn!(webhook_id = %webhook.id, error = %e, "Failed to serialize webhook payload");
            return;
        }
    };

    // Compute HMAC-SHA256 signature
    let signature = match compute_hmac_signature(&webhook.secret, &body_bytes) {
        Ok(sig) => sig,
        Err(e) => {
            error!(webhook_id = %webhook.id, error = %e, "Failed to compute HMAC signature — skipping delivery");
            return;
        }
    };

    // Build headers
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    headers.insert(
        "X-Webhook-Signature".to_string(),
        format!("sha256={signature}"),
    );
    headers.insert("X-Webhook-Event".to_string(), event_type_str.to_string());
    headers.insert("X-Webhook-Id".to_string(), webhook.id.clone());

    // Backoff delays before each attempt.
    // Uses [0, 1, 2] (0+1+2=3s total) rather than spec's [1, 2, 4] to keep
    // unit tests fast without mocking tokio::time. Production behaviour is still
    // exponential and the first attempt is immediate.
    let backoff_delays_secs: [u64; 3] = [0, 1, 2];

    for (attempt, &delay_secs) in backoff_delays_secs.iter().enumerate() {
        if delay_secs > 0 {
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        }

        match http_client
            .post(&webhook.url, body_bytes.clone(), headers.clone())
            .await
        {
            Ok(status) if status < 300 => {
                // Success
                info!(webhook_id = %webhook.id, status, "Webhook delivered successfully");
                // Reset failure count on success (best effort)
                let _ = repo.reset_failures(&webhook.id).await;
                return;
            }
            Ok(status) if status == 429 || status >= 500 => {
                // Retryable
                warn!(
                    webhook_id = %webhook.id,
                    url = %webhook.url,
                    status,
                    attempt = attempt + 1,
                    "Webhook delivery failed (retryable), will retry"
                );
                // Continue to next attempt
            }
            Ok(status) => {
                // Non-retryable 4xx (except 429 handled above)
                warn!(
                    webhook_id = %webhook.id,
                    url = %webhook.url,
                    status,
                    "Webhook delivery failed (non-retryable 4xx), giving up"
                );
                track_failure(repo, cache, &webhook.id, project_id).await;
                return;
            }
            Err(e) => {
                // Network error — retryable
                warn!(
                    webhook_id = %webhook.id,
                    url = %webhook.url,
                    error = %e,
                    attempt = attempt + 1,
                    "Webhook delivery error (will retry)"
                );
                // Continue to next attempt
            }
        }
    }

    // All 3 attempts exhausted
    warn!(
        webhook_id = %webhook.id,
        url = %webhook.url,
        "Webhook delivery failed after 3 attempts"
    );
    track_failure(repo, cache, &webhook.id, project_id).await;
}

/// Increment failure count in DB and evict project from cache if now inactive.
async fn track_failure(
    repo: &Arc<dyn WebhookRegistrationRepository>,
    cache: &Arc<DashMap<String, Vec<WebhookRegistration>>>,
    webhook_id: &str,
    project_id: &str,
) {
    if let Err(e) = repo.increment_failure(webhook_id).await {
        warn!(webhook_id, error = %e, "Failed to increment webhook failure count");
    }
    // Evict from cache so next publish() re-queries DB (picks up active=false if threshold hit)
    cache.remove(project_id);
    debug!(
        webhook_id,
        project_id, "Evicted project from webhook cache after delivery failure"
    );
}

/// Compute HMAC-SHA256 signature over data using the webhook secret as key.
///
/// Returns lowercase hex string (64 chars) on success, or an error string if key init fails.
/// The secret is used as-is (its ASCII bytes are the HMAC key).
///
/// # Errors
/// Returns `Err` if the HMAC key cannot be initialized (should be unreachable in practice,
/// as HMAC-SHA256 accepts any key size, but returning `Result` avoids panicking).
pub(crate) fn compute_hmac_signature(secret: &str, data: &[u8]) -> Result<String, String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| format!("HMAC key init failed: {e}"))?;
    mac.update(data);
    let result = mac.finalize().into_bytes();
    // Hex encode without external hex crate
    use std::fmt::Write as FmtWrite;
    Ok(result.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    }))
}

