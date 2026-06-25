// Memory-based WebhookRegistrationRepository implementation for testing
// Uses RwLock<HashMap> for thread-safe storage

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use async_trait::async_trait;

use crate::domain::repositories::{WebhookRegistration, WebhookRegistrationRepository};
use crate::error::AppResult;

/// In-memory implementation of WebhookRegistrationRepository for testing
pub struct MemoryWebhookRegistrationRepository {
    store: Arc<RwLock<HashMap<String, WebhookRegistration>>>,
}

impl Default for MemoryWebhookRegistrationRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryWebhookRegistrationRepository {
    /// Create a new empty in-memory webhook registration repository
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl WebhookRegistrationRepository for MemoryWebhookRegistrationRepository {
    async fn upsert(&self, registration: WebhookRegistration) -> AppResult<WebhookRegistration> {
        let mut store = self.store.write().await;
        // Check for existing by url+api_key_id
        let existing_id = store
            .values()
            .find(|r| r.url == registration.url && r.api_key_id == registration.api_key_id)
            .map(|r| r.id.clone());
        if let Some(id) = existing_id {
            let entry = store.get_mut(&id).unwrap();
            entry.project_ids = registration.project_ids.clone();
            entry.event_types = registration.event_types.clone();
            entry.failure_count = 0;
            entry.active = true;
            Ok(entry.clone())
        } else {
            store.insert(registration.id.clone(), registration.clone());
            Ok(registration)
        }
    }

    async fn get_by_id(&self, id: &str) -> AppResult<Option<WebhookRegistration>> {
        let store = self.store.read().await;
        Ok(store.get(id).cloned())
    }

    async fn get_by_url_and_key(
        &self,
        url: &str,
        api_key_id: &str,
    ) -> AppResult<Option<WebhookRegistration>> {
        let store = self.store.read().await;
        Ok(store
            .values()
            .find(|r| r.url == url && r.api_key_id == api_key_id)
            .cloned())
    }

    async fn list_by_api_key(&self, api_key_id: &str) -> AppResult<Vec<WebhookRegistration>> {
        let store = self.store.read().await;
        let mut results: Vec<_> = store
            .values()
            .filter(|r| r.api_key_id == api_key_id)
            .cloned()
            .collect();
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(results)
    }

    async fn deactivate(&self, id: &str, api_key_id: &str) -> AppResult<bool> {
        let mut store = self.store.write().await;
        if let Some(entry) = store.get_mut(id) {
            if entry.api_key_id == api_key_id {
                entry.active = false;
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn increment_failure(&self, id: &str) -> AppResult<()> {
        let mut store = self.store.write().await;
        if let Some(entry) = store.get_mut(id) {
            entry.failure_count += 1;
            if entry.failure_count >= 10 {
                entry.active = false;
            }
        }
        Ok(())
    }

    async fn reset_failures(&self, id: &str) -> AppResult<()> {
        let mut store = self.store.write().await;
        if let Some(entry) = store.get_mut(id) {
            entry.failure_count = 0;
            entry.active = true;
        }
        Ok(())
    }

    async fn list_active_for_project(
        &self,
        project_id: &str,
    ) -> AppResult<Vec<WebhookRegistration>> {
        let store = self.store.read().await;
        let project_id = project_id.to_string();
        let mut results: Vec<_> = store
            .values()
            .filter(|r| {
                if !r.active {
                    return false;
                }
                // project_ids is a JSON array like ["proj-1", "proj-2"]
                // Empty array '[]' means match all projects
                serde_json::from_str::<Vec<String>>(&r.project_ids)
                    .map(|ids| ids.is_empty() || ids.contains(&project_id))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(results)
    }
}

#[cfg(test)]
#[path = "memory_webhook_registration_repo_tests.rs"]
mod tests;
