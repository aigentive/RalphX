use async_trait::async_trait;

use crate::agents::{AgentHarnessKind, AgentProviderSettings};

use super::AgentProviderSettingsRepository;

#[derive(Default)]
struct MockAgentProviderSettingsRepository {
    rows: Vec<AgentProviderSettings>,
}

#[async_trait]
impl AgentProviderSettingsRepository for MockAgentProviderSettingsRepository {
    async fn get(
        &self,
        provider: AgentHarnessKind,
    ) -> Result<Option<AgentProviderSettings>, Box<dyn std::error::Error>> {
        Ok(self
            .rows
            .iter()
            .find(|row| row.provider == provider)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<AgentProviderSettings>, Box<dyn std::error::Error>> {
        Ok(self.rows.clone())
    }

    async fn get_default(
        &self,
    ) -> Result<Option<AgentProviderSettings>, Box<dyn std::error::Error>> {
        Ok(self.rows.iter().find(|row| row.is_default).cloned())
    }

    async fn upsert(
        &self,
        settings: &AgentProviderSettings,
    ) -> Result<AgentProviderSettings, Box<dyn std::error::Error>> {
        Ok(settings.clone())
    }
}

#[test]
fn trait_object_is_send_sync() {
    let repo: std::sync::Arc<dyn AgentProviderSettingsRepository> =
        std::sync::Arc::new(MockAgentProviderSettingsRepository::default());
    drop(repo);
}
