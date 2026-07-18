use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::entities::UiFeatureFlagOverrides;
use crate::domain::repositories::UiFeatureFlagOverridesRepository;
use crate::error::AppResult;

pub struct MemoryUiFeatureFlagOverridesRepository {
    overrides: Arc<RwLock<UiFeatureFlagOverrides>>,
}

impl Default for MemoryUiFeatureFlagOverridesRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryUiFeatureFlagOverridesRepository {
    pub fn new() -> Self {
        Self {
            overrides: Arc::new(RwLock::new(UiFeatureFlagOverrides::default())),
        }
    }
}

#[async_trait]
impl UiFeatureFlagOverridesRepository for MemoryUiFeatureFlagOverridesRepository {
    async fn get(&self) -> AppResult<UiFeatureFlagOverrides> {
        Ok(self.overrides.read().await.clone())
    }

    async fn set_agent_personas(&self, value: Option<bool>) -> AppResult<()> {
        self.overrides.write().await.agent_personas = value;
        Ok(())
    }

    async fn update_agent_capabilities(
        &self,
        team: Option<bool>,
        workflows: Option<bool>,
        autopilot: Option<bool>,
    ) -> AppResult<UiFeatureFlagOverrides> {
        let mut overrides = self.overrides.write().await;
        if let Some(team) = team {
            overrides.agent_conversation_team = team;
        }
        if let Some(workflows) = workflows {
            overrides.agent_conversation_workflows = workflows;
        }
        if let Some(autopilot) = autopilot {
            overrides.agent_conversation_autopilot = autopilot;
        }
        Ok(overrides.clone())
    }
}
