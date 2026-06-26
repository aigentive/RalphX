// Memory-based ApiKeyRepository implementation for testing
// Uses RwLock<HashMap> for thread-safe storage

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use async_trait::async_trait;

use crate::domain::entities::{ApiKey, ApiKeyId, AuditLogEntry};
use crate::domain::repositories::{ApiKeyRepository, CreateKeyParams, RotateKeyParams};
use crate::error::AppResult;

/// In-memory implementation of ApiKeyRepository for testing
/// Uses RwLock<HashMap> for thread-safe storage
pub struct MemoryApiKeyRepository {
    keys: Arc<RwLock<HashMap<String, ApiKey>>>,
    /// project associations: api_key_id -> Vec<project_id>
    projects: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// audit log entries (api_key_id, tool_name, project_id, success, latency_ms)
    audit_log: Arc<RwLock<Vec<(String, String, Option<String>, bool, Option<i64>)>>>,
}

impl Default for MemoryApiKeyRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryApiKeyRepository {
    /// Create a new empty in-memory API key repository
    pub fn new() -> Self {
        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
            projects: Arc::new(RwLock::new(HashMap::new())),
            audit_log: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl ApiKeyRepository for MemoryApiKeyRepository {
    async fn create(&self, api_key: ApiKey) -> AppResult<ApiKey> {
        let mut keys = self.keys.write().await;
        keys.insert(api_key.id.as_str().to_string(), api_key.clone());
        Ok(api_key)
    }

    async fn list(&self) -> AppResult<Vec<ApiKey>> {
        let keys = self.keys.read().await;
        let mut result: Vec<ApiKey> = keys
            .values()
            .filter(|k| k.revoked_at.is_none())
            .cloned()
            .collect();
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(result)
    }

    async fn get_by_id(&self, id: &ApiKeyId) -> AppResult<Option<ApiKey>> {
        let keys = self.keys.read().await;
        Ok(keys.get(id.as_str()).cloned())
    }

    async fn get_by_hash(&self, key_hash: &str) -> AppResult<Option<ApiKey>> {
        let keys = self.keys.read().await;
        Ok(keys.values().find(|k| k.key_hash == key_hash).cloned())
    }

    async fn revoke(&self, id: &ApiKeyId) -> AppResult<()> {
        let mut keys = self.keys.write().await;
        if let Some(key) = keys.get_mut(id.as_str()) {
            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            key.revoked_at = Some(now);
        }
        Ok(())
    }

    async fn set_grace_period(&self, id: &ApiKeyId, grace_expires_at: &str) -> AppResult<()> {
        let mut keys = self.keys.write().await;
        if let Some(key) = keys.get_mut(id.as_str()) {
            key.grace_expires_at = Some(grace_expires_at.to_string());
        }
        Ok(())
    }

    async fn update_last_used(&self, id: &ApiKeyId, timestamp: &str) -> AppResult<()> {
        let mut keys = self.keys.write().await;
        if let Some(key) = keys.get_mut(id.as_str()) {
            key.last_used_at = Some(timestamp.to_string());
        }
        Ok(())
    }

    async fn get_projects(&self, id: &ApiKeyId) -> AppResult<Vec<String>> {
        let projects = self.projects.read().await;
        Ok(projects.get(id.as_str()).cloned().unwrap_or_default())
    }

    async fn set_projects(&self, id: &ApiKeyId, project_ids: &[String]) -> AppResult<()> {
        let mut projects = self.projects.write().await;
        projects.insert(id.as_str().to_string(), project_ids.to_vec());
        Ok(())
    }

    async fn log_audit(
        &self,
        api_key_id: &str,
        tool_name: &str,
        project_id: Option<&str>,
        success: bool,
        latency_ms: Option<i64>,
    ) -> AppResult<()> {
        let mut log = self.audit_log.write().await;
        log.push((
            api_key_id.to_string(),
            tool_name.to_string(),
            project_id.map(|s| s.to_string()),
            success,
            latency_ms,
        ));
        Ok(())
    }

    async fn get_audit_log(
        &self,
        key_id: &str,
        limit: Option<i64>,
    ) -> AppResult<Vec<AuditLogEntry>> {
        let log = self.audit_log.read().await;
        let limit = limit.unwrap_or(100) as usize;
        let entries: Vec<AuditLogEntry> = log
            .iter()
            .filter(|(id, _, _, _, _)| id == key_id)
            .rev()
            .take(limit)
            .enumerate()
            .map(
                |(i, (api_key_id, tool_name, project_id, success, latency_ms))| AuditLogEntry {
                    id: i as i64,
                    api_key_id: api_key_id.clone(),
                    tool_name: tool_name.clone(),
                    project_id: project_id.clone(),
                    success: *success,
                    latency_ms: *latency_ms,
                    created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                },
            )
            .collect();
        Ok(entries)
    }

    async fn update_api_key_permissions(&self, key_id: &str, permissions: i64) -> AppResult<()> {
        let mut keys = self.keys.write().await;
        if let Some(key) = keys.get_mut(key_id) {
            key.permissions = permissions as i32;
        }
        Ok(())
    }

    async fn create_key_atomic(&self, params: CreateKeyParams) -> AppResult<ApiKey> {
        // In-memory implementation: apply each step sequentially (no real transaction needed).
        let new_key_id = params.new_key.id.clone();
        let created = self.create(params.new_key).await?;
        if !params.project_ids.is_empty() {
            self.set_projects(&new_key_id, &params.project_ids).await?;
        }
        Ok(created)
    }

    async fn rotate_key_atomic(&self, params: RotateKeyParams) -> AppResult<()> {
        // In-memory implementation: apply each step sequentially (no real transaction needed).
        let new_key_id = params.new_key.id.clone();
        self.create(params.new_key).await?;
        if !params.project_ids.is_empty() {
            self.set_projects(&new_key_id, &params.project_ids).await?;
        }
        // Revoke the old key and set its grace period so it is technically revoked
        // but still usable via is_in_grace_period() until the grace window elapses.
        self.revoke(&params.old_key_id).await?;
        self.set_grace_period(&params.old_key_id, &params.grace_expires_at)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_api_key_repo_tests.rs"]
mod tests;
