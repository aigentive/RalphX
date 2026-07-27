// Memory-based RemoteEnvironmentRepository implementation for testing.
// Mirrors the SQLite semantics: environment_id dedup merge, pending_add inserts,
// NotFound on status writes to missing rows, idempotent delete.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::entities::remote_environment::{
    remote_environment_token_secret_ref, RemoteEnvironment, RemoteEnvironmentId,
    RemoteEnvironmentStatus,
};
use crate::domain::repositories::{RemoteEnvironmentRepository, UpsertPairedEnvironment};
use crate::error::{AppError, AppResult};

pub struct MemoryRemoteEnvironmentRepository {
    environments: Arc<RwLock<HashMap<String, RemoteEnvironment>>>,
}

impl Default for MemoryRemoteEnvironmentRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryRemoteEnvironmentRepository {
    pub fn new() -> Self {
        Self {
            environments: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl RemoteEnvironmentRepository for MemoryRemoteEnvironmentRepository {
    async fn upsert_paired(
        &self,
        params: UpsertPairedEnvironment,
    ) -> AppResult<RemoteEnvironment> {
        let mut environments = self.environments.write().await;
        let existing = environments
            .values()
            .find(|env| env.environment_id == params.environment_id)
            .cloned();
        let env = match existing {
            Some(mut env) => {
                if params.url != env.base_url && !env.candidate_urls.contains(&params.url) {
                    env.candidate_urls.push(params.url.clone());
                }
                env.name = params.name;
                env.scopes = params.scopes;
                env.protocol_version = params.protocol_version;
                env.status = RemoteEnvironmentStatus::PendingAdd;
                env
            }
            None => {
                let id = RemoteEnvironmentId::new();
                let token_secret_ref = remote_environment_token_secret_ref(&id);
                RemoteEnvironment {
                    id,
                    environment_id: params.environment_id,
                    name: params.name,
                    base_url: params.url,
                    candidate_urls: Vec::new(),
                    token_secret_ref,
                    scopes: params.scopes,
                    protocol_version: params.protocol_version,
                    status: RemoteEnvironmentStatus::PendingAdd,
                    created_at: Utc::now().format("%Y-%m-%dT%H:%M:%S+00:00").to_string(),
                    last_connected_at: None,
                }
            }
        };
        environments.insert(env.id.as_str().to_string(), env.clone());
        Ok(env)
    }

    async fn get(&self, id: &RemoteEnvironmentId) -> AppResult<Option<RemoteEnvironment>> {
        let environments = self.environments.read().await;
        Ok(environments.get(id.as_str()).cloned())
    }

    async fn get_by_environment_id(
        &self,
        environment_id: &str,
    ) -> AppResult<Option<RemoteEnvironment>> {
        let environments = self.environments.read().await;
        Ok(environments
            .values()
            .find(|env| env.environment_id == environment_id)
            .cloned())
    }

    async fn list(&self) -> AppResult<Vec<RemoteEnvironment>> {
        let environments = self.environments.read().await;
        let mut all: Vec<RemoteEnvironment> = environments.values().cloned().collect();
        all.sort_by(|a, b| {
            (a.created_at.as_str(), a.id.as_str()).cmp(&(b.created_at.as_str(), b.id.as_str()))
        });
        Ok(all)
    }

    async fn set_status(
        &self,
        id: &RemoteEnvironmentId,
        status: RemoteEnvironmentStatus,
    ) -> AppResult<()> {
        let mut environments = self.environments.write().await;
        match environments.get_mut(id.as_str()) {
            Some(env) => {
                env.status = status;
                Ok(())
            }
            None => Err(AppError::NotFound(format!(
                "remote environment not found: {}",
                id.as_str()
            ))),
        }
    }

    async fn delete(&self, id: &RemoteEnvironmentId) -> AppResult<()> {
        let mut environments = self.environments.write().await;
        environments.remove(id.as_str());
        Ok(())
    }

    async fn touch_last_connected(
        &self,
        id: &RemoteEnvironmentId,
        timestamp: &str,
    ) -> AppResult<()> {
        let mut environments = self.environments.write().await;
        if let Some(env) = environments.get_mut(id.as_str()) {
            env.last_connected_at = Some(timestamp.to_string());
        }
        Ok(())
    }
}
