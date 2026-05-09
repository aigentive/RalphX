use async_trait::async_trait;

use crate::agents::{AgentHarnessKind, AgentProviderSettings};

#[async_trait]
pub trait AgentProviderSettingsRepository: Send + Sync {
    async fn get(
        &self,
        provider: AgentHarnessKind,
    ) -> Result<Option<AgentProviderSettings>, Box<dyn std::error::Error>>;

    async fn list(&self) -> Result<Vec<AgentProviderSettings>, Box<dyn std::error::Error>>;

    async fn get_default(
        &self,
    ) -> Result<Option<AgentProviderSettings>, Box<dyn std::error::Error>>;

    async fn upsert(
        &self,
        settings: &AgentProviderSettings,
    ) -> Result<AgentProviderSettings, Box<dyn std::error::Error>>;
}

#[cfg(test)]
#[path = "agent_provider_settings_repository_tests.rs"]
mod tests;
