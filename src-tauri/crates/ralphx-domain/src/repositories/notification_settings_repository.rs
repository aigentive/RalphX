use async_trait::async_trait;

use crate::entities::NotificationSettings;
use crate::error::AppResult;

#[async_trait]
pub trait NotificationSettingsRepository: Send + Sync {
    /// Returns product defaults until the singleton row is persisted.
    async fn get_settings(&self) -> AppResult<NotificationSettings>;
    async fn update_settings(
        &self,
        settings: &NotificationSettings,
    ) -> AppResult<NotificationSettings>;
}
