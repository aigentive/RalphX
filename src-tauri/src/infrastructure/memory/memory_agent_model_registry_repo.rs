use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::agents::{AgentHarnessKind, AgentModelDefinition, AgentModelSource};
use crate::domain::repositories::AgentModelRegistryRepository;

type ModelKey = (AgentHarnessKind, String);

pub struct MemoryAgentModelRegistryRepository {
    models: Arc<RwLock<HashMap<ModelKey, AgentModelDefinition>>>,
}

impl Default for MemoryAgentModelRegistryRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryAgentModelRegistryRepository {
    pub fn new() -> Self {
        Self {
            models: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl AgentModelRegistryRepository for MemoryAgentModelRegistryRepository {
    async fn list_custom_models(
        &self,
    ) -> Result<Vec<AgentModelDefinition>, Box<dyn std::error::Error>> {
        let mut rows: Vec<_> = self.models.read().await.values().cloned().collect();
        rows.sort_by_key(|model| (model.provider.to_string(), model.model_id.clone()));
        Ok(rows)
    }

    async fn upsert_custom_model(
        &self,
        model: &AgentModelDefinition,
    ) -> Result<AgentModelDefinition, Box<dyn std::error::Error>> {
        let mut model = model.clone().normalized();
        model.source = AgentModelSource::Custom;
        let now = Utc::now();
        let key = (model.provider, model.model_id.clone());
        let created_at = self
            .models
            .read()
            .await
            .get(&key)
            .and_then(|existing| existing.created_at)
            .unwrap_or(now);
        model.created_at = Some(created_at);
        model.updated_at = Some(now);
        self.models.write().await.insert(key, model.clone());
        Ok(model)
    }

    async fn delete_custom_model(
        &self,
        provider: AgentHarnessKind,
        model_id: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(self
            .models
            .write()
            .await
            .remove(&(provider, model_id.to_string()))
            .is_some())
    }
}
