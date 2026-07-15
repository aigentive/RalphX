use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::entities::UiFeatureFlagOverrides;
use crate::domain::repositories::UiFeatureFlagOverridesRepository;
use crate::error::AppResult;

pub struct MemoryUiFeatureFlagOverridesRepository {
    agent_personas: Arc<RwLock<Option<bool>>>,
}

impl Default for MemoryUiFeatureFlagOverridesRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryUiFeatureFlagOverridesRepository {
    pub fn new() -> Self {
        Self {
            agent_personas: Arc::new(RwLock::new(None)),
        }
    }
}

#[async_trait]
impl UiFeatureFlagOverridesRepository for MemoryUiFeatureFlagOverridesRepository {
    async fn get(&self) -> AppResult<UiFeatureFlagOverrides> {
        Ok(UiFeatureFlagOverrides {
            agent_personas: *self.agent_personas.read().await,
        })
    }

    async fn set_agent_personas(&self, value: Option<bool>) -> AppResult<()> {
        *self.agent_personas.write().await = value;
        Ok(())
    }
}
