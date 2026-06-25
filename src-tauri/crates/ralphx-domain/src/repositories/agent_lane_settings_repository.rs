use async_trait::async_trait;

use crate::agents::{AgentLane, AgentLaneSettings, StoredAgentLaneSettings};

#[async_trait]
pub trait AgentLaneSettingsRepository: Send + Sync {
    async fn get_global(
        &self,
        lane: AgentLane,
    ) -> Result<Option<StoredAgentLaneSettings>, Box<dyn std::error::Error>>;

    async fn get_for_project(
        &self,
        project_id: &str,
        lane: AgentLane,
    ) -> Result<Option<StoredAgentLaneSettings>, Box<dyn std::error::Error>>;

    async fn list_global(&self)
        -> Result<Vec<StoredAgentLaneSettings>, Box<dyn std::error::Error>>;

    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<StoredAgentLaneSettings>, Box<dyn std::error::Error>>;

    async fn upsert_global(
        &self,
        lane: AgentLane,
        settings: &AgentLaneSettings,
    ) -> Result<StoredAgentLaneSettings, Box<dyn std::error::Error>>;

    async fn upsert_for_project(
        &self,
        project_id: &str,
        lane: AgentLane,
        settings: &AgentLaneSettings,
    ) -> Result<StoredAgentLaneSettings, Box<dyn std::error::Error>>;
}

#[cfg(test)]
#[path = "agent_lane_settings_repository_tests.rs"]
mod tests;
