use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::agents::{AgentLane, AgentLaneSettings, StoredAgentLaneSettings};
use crate::domain::repositories::AgentLaneSettingsRepository;

pub struct MemoryAgentLaneSettingsRepository {
    next_id: Arc<RwLock<i64>>,
    global_rows: Arc<RwLock<HashMap<AgentLane, StoredAgentLaneSettings>>>,
    project_rows: Arc<RwLock<HashMap<(String, AgentLane), StoredAgentLaneSettings>>>,
}

impl Default for MemoryAgentLaneSettingsRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryAgentLaneSettingsRepository {
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
impl AgentLaneSettingsRepository for MemoryAgentLaneSettingsRepository {
    async fn get_global(
        &self,
        lane: AgentLane,
    ) -> Result<Option<StoredAgentLaneSettings>, Box<dyn std::error::Error>> {
        Ok(self.global_rows.read().await.get(&lane).cloned())
    }

    async fn get_for_project(
        &self,
        project_id: &str,
        lane: AgentLane,
    ) -> Result<Option<StoredAgentLaneSettings>, Box<dyn std::error::Error>> {
        Ok(self
            .project_rows
            .read()
            .await
            .get(&(project_id.to_string(), lane))
            .cloned())
    }

    async fn list_global(
        &self,
    ) -> Result<Vec<StoredAgentLaneSettings>, Box<dyn std::error::Error>> {
        let mut rows: Vec<_> = self.global_rows.read().await.values().cloned().collect();
        rows.sort_by_key(|row| row.lane.to_string());
        Ok(rows)
    }

    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<StoredAgentLaneSettings>, Box<dyn std::error::Error>> {
        let mut rows: Vec<_> = self
            .project_rows
            .read()
            .await
            .iter()
            .filter(|((pid, _), _)| pid == project_id)
            .map(|(_, row)| row.clone())
            .collect();
        rows.sort_by_key(|row| row.lane.to_string());
        Ok(rows)
    }

    async fn upsert_global(
        &self,
        lane: AgentLane,
        settings: &AgentLaneSettings,
    ) -> Result<StoredAgentLaneSettings, Box<dyn std::error::Error>> {
        let id = self
            .global_rows
            .read()
            .await
            .get(&lane)
            .map(|row| row.id)
            .unwrap_or(self.allocate_id().await);

        let row = StoredAgentLaneSettings {
            id,
            project_id: None,
            lane,
            settings: settings.clone(),
            updated_at: Utc::now(),
        };
        self.global_rows.write().await.insert(lane, row.clone());
        Ok(row)
    }

    async fn upsert_for_project(
        &self,
        project_id: &str,
        lane: AgentLane,
        settings: &AgentLaneSettings,
    ) -> Result<StoredAgentLaneSettings, Box<dyn std::error::Error>> {
        let key = (project_id.to_string(), lane);
        let id = self
            .project_rows
            .read()
            .await
            .get(&key)
            .map(|row| row.id)
            .unwrap_or(self.allocate_id().await);

        let row = StoredAgentLaneSettings {
            id,
            project_id: Some(project_id.to_string()),
            lane,
            settings: settings.clone(),
            updated_at: Utc::now(),
        };
        self.project_rows.write().await.insert(key, row.clone());
        Ok(row)
    }
}

#[cfg(test)]
#[path = "memory_agent_lane_settings_repo_tests.rs"]
mod tests;
