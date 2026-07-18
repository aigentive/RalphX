use async_trait::async_trait;

use crate::domain::entities::UiFeatureFlagOverrides;
use crate::error::AppResult;

#[async_trait]
pub trait UiFeatureFlagOverridesRepository: Send + Sync {
    async fn get(&self) -> AppResult<UiFeatureFlagOverrides>;

    async fn set_agent_personas(&self, value: Option<bool>) -> AppResult<()>;

    async fn update_agent_capabilities(
        &self,
        team: Option<bool>,
        workflows: Option<bool>,
        autopilot: Option<bool>,
    ) -> AppResult<UiFeatureFlagOverrides>;
}
