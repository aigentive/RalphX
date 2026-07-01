use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::agents::{
    AgentHarnessKind, StoredWorkspaceReviewRuntimeSettings, WorkspaceReviewRuntimeSettings,
};
use crate::domain::repositories::WorkspaceReviewRuntimeSettingsRepository;

pub struct MemoryWorkspaceReviewRuntimeSettingsRepository {
    next_id: Arc<RwLock<i64>>,
    global_rows: Arc<RwLock<HashMap<AgentHarnessKind, StoredWorkspaceReviewRuntimeSettings>>>,
    project_rows:
        Arc<RwLock<HashMap<(String, AgentHarnessKind), StoredWorkspaceReviewRuntimeSettings>>>,
}

impl Default for MemoryWorkspaceReviewRuntimeSettingsRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryWorkspaceReviewRuntimeSettingsRepository {
    pub fn new() -> Self {
        Self {
            next_id: Arc::new(RwLock::new(1)),
            global_rows: Arc::new(RwLock::new(HashMap::new())),
            project_rows: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn allocate_id(&self) -> i64 {
        let mut next_id = self.next_id.write().await;
        let id = *next_id;
        *next_id += 1;
        id
    }
}

#[async_trait]
impl WorkspaceReviewRuntimeSettingsRepository for MemoryWorkspaceReviewRuntimeSettingsRepository {
    async fn get_global(
        &self,
        provider: AgentHarnessKind,
    ) -> Result<Option<StoredWorkspaceReviewRuntimeSettings>, Box<dyn std::error::Error>> {
        Ok(self.global_rows.read().await.get(&provider).cloned())
    }

    async fn get_for_project(
        &self,
        project_id: &str,
        provider: AgentHarnessKind,
    ) -> Result<Option<StoredWorkspaceReviewRuntimeSettings>, Box<dyn std::error::Error>> {
        Ok(self
            .project_rows
            .read()
            .await
            .get(&(project_id.to_string(), provider))
            .cloned())
    }

    async fn list_global(
        &self,
    ) -> Result<Vec<StoredWorkspaceReviewRuntimeSettings>, Box<dyn std::error::Error>> {
        let mut rows: Vec<_> = self.global_rows.read().await.values().cloned().collect();
        rows.sort_by_key(|row| row.provider.to_string());
        Ok(rows)
    }

    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<StoredWorkspaceReviewRuntimeSettings>, Box<dyn std::error::Error>> {
        let mut rows: Vec<_> = self
            .project_rows
            .read()
            .await
            .iter()
            .filter(|((pid, _), _)| pid == project_id)
            .map(|(_, row)| row.clone())
            .collect();
        rows.sort_by_key(|row| row.provider.to_string());
        Ok(rows)
    }

    async fn upsert_global(
        &self,
        provider: AgentHarnessKind,
        settings: &WorkspaceReviewRuntimeSettings,
    ) -> Result<StoredWorkspaceReviewRuntimeSettings, Box<dyn std::error::Error>> {
        let id = self
            .global_rows
            .read()
            .await
            .get(&provider)
            .map(|row| row.id)
            .unwrap_or(self.allocate_id().await);

        let row = StoredWorkspaceReviewRuntimeSettings {
            id,
            project_id: None,
            provider,
            settings: settings.clone(),
            updated_at: Utc::now(),
        };
        self.global_rows.write().await.insert(provider, row.clone());
        Ok(row)
    }

    async fn upsert_for_project(
        &self,
        project_id: &str,
        provider: AgentHarnessKind,
        settings: &WorkspaceReviewRuntimeSettings,
    ) -> Result<StoredWorkspaceReviewRuntimeSettings, Box<dyn std::error::Error>> {
        let key = (project_id.to_string(), provider);
        let id = self
            .project_rows
            .read()
            .await
            .get(&key)
            .map(|row| row.id)
            .unwrap_or(self.allocate_id().await);

        let row = StoredWorkspaceReviewRuntimeSettings {
            id,
            project_id: Some(project_id.to_string()),
            provider,
            settings: settings.clone(),
            updated_at: Utc::now(),
        };
        self.project_rows.write().await.insert(key, row.clone());
        Ok(row)
    }
}

#[cfg(test)]
#[path = "memory_workspace_review_runtime_settings_repo_tests.rs"]
mod tests;
