use async_trait::async_trait;

use crate::domain::entities::UiFeatureFlagOverrides;
use crate::error::AppResult;

#[async_trait]
pub trait UiFeatureFlagOverridesRepository: Send + Sync {
    async fn get(&self) -> AppResult<UiFeatureFlagOverrides>;

    async fn set_agent_personas(&self, value: Option<bool>) -> AppResult<()>;
}
