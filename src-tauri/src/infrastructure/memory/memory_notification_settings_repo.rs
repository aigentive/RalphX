use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::entities::NotificationSettings;
use crate::domain::repositories::NotificationSettingsRepository;
use crate::error::AppResult;

pub struct MemoryNotificationSettingsRepository {
    settings: Arc<RwLock<NotificationSettings>>,
}

impl Default for MemoryNotificationSettingsRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryNotificationSettingsRepository {
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(NotificationSettings::default())),
        }
    }
}

#[async_trait]
impl NotificationSettingsRepository for MemoryNotificationSettingsRepository {
    async fn get_settings(&self) -> AppResult<NotificationSettings> {
        Ok(self.settings.read().await.clone())
    }

    async fn update_settings(
        &self,
        settings: &NotificationSettings,
    ) -> AppResult<NotificationSettings> {
        *self.settings.write().await = settings.clone();
        Ok(settings.clone())
    }
}
