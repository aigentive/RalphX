// Memory-based IdeationSettingsRepository implementation for testing
// Uses RwLock for thread-safe storage without a real database

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::domain::ideation::{IdeationSettings, TasksFeatureState};
use crate::domain::repositories::IdeationSettingsRepository;

/// In-memory implementation of IdeationSettingsRepository for testing
/// Uses RwLock for thread-safe storage
pub struct MemoryIdeationSettingsRepository {
    settings: Arc<RwLock<IdeationSettings>>,
}

impl Default for MemoryIdeationSettingsRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryIdeationSettingsRepository {
    /// Create a new empty in-memory ideation settings repository
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(IdeationSettings::default())),
        }
    }

    /// Create with specific settings (for tests)
    pub fn with_settings(settings: IdeationSettings) -> Self {
        let mut settings = settings;
        settings.tasks_enabled = settings.tasks_feature_state.tasks_enabled();
        Self {
            settings: Arc::new(RwLock::new(settings)),
        }
    }
}

#[async_trait]
impl IdeationSettingsRepository for MemoryIdeationSettingsRepository {
    async fn get_settings(&self) -> Result<IdeationSettings, Box<dyn std::error::Error>> {
        let settings = self.settings.read().await;
        Ok(settings.clone())
    }

    async fn update_settings(
        &self,
        new_settings: &IdeationSettings,
    ) -> Result<IdeationSettings, Box<dyn std::error::Error>> {
        let mut settings = self.settings.write().await;
        let feature_state = settings.tasks_feature_state;
        *settings = new_settings.clone();
        settings.tasks_feature_state = feature_state;
        settings.tasks_enabled = feature_state.tasks_enabled();
        Ok(settings.clone())
    }

    async fn compare_and_set_tasks_feature_state(
        &self,
        expected: TasksFeatureState,
        next: TasksFeatureState,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let mut settings = self.settings.write().await;
        if settings.tasks_feature_state != expected {
            return Ok(false);
        }
        settings.tasks_feature_state = next;
        settings.tasks_enabled = next.tasks_enabled();
        Ok(true)
    }
}

#[cfg(test)]
#[path = "memory_ideation_settings_repo_tests.rs"]
mod tests;
