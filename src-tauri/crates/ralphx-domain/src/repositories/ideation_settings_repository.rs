use crate::domain::ideation::{IdeationSettings, TasksFeatureState};
use async_trait::async_trait;

#[async_trait]
pub trait IdeationSettingsRepository: Send + Sync {
    /// Get ideation settings (returns default if no settings exist)
    async fn get_settings(&self) -> Result<IdeationSettings, Box<dyn std::error::Error>>;

    /// Update ideation settings
    async fn update_settings(
        &self,
        settings: &IdeationSettings,
    ) -> Result<IdeationSettings, Box<dyn std::error::Error>>;

    /// Atomically move the backend-owned Tasks feature state and mirrored legacy boolean.
    async fn compare_and_set_tasks_feature_state(
        &self,
        expected: TasksFeatureState,
        next: TasksFeatureState,
    ) -> Result<bool, Box<dyn std::error::Error>>;
}
