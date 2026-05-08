use async_trait::async_trait;

use crate::agents::{AgentHarnessKind, AgentModelDefinition};

#[async_trait]
pub trait AgentModelRegistryRepository: Send + Sync {
    async fn list_custom_models(
        &self,
    ) -> Result<Vec<AgentModelDefinition>, Box<dyn std::error::Error>>;

    async fn upsert_custom_model(
        &self,
        model: &AgentModelDefinition,
    ) -> Result<AgentModelDefinition, Box<dyn std::error::Error>>;

    async fn delete_custom_model(
        &self,
        provider: AgentHarnessKind,
        model_id: &str,
    ) -> Result<bool, Box<dyn std::error::Error>>;
}
