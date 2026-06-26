use crate::domain::entities::ProjectId;
use crate::domain::execution::{ExecutionSettings, GlobalExecutionSettings};
use async_trait::async_trait;

/// Repository for per-project execution settings
/// Phase 82: Extended to support project-specific settings with optional project_id
#[async_trait]
pub trait ExecutionSettingsRepository: Send + Sync {
    /// Get execution settings for a project (returns default if no settings exist)
    /// If project_id is None, returns global defaults
    async fn get_settings(
        &self,
        project_id: Option<&ProjectId>,
    ) -> Result<ExecutionSettings, Box<dyn std::error::Error>>;

    /// Update execution settings for a project
    /// If project_id is None, updates global defaults
    async fn update_settings(
        &self,
        project_id: Option<&ProjectId>,
        settings: &ExecutionSettings,
    ) -> Result<ExecutionSettings, Box<dyn std::error::Error>>;
}

/// Repository for global execution settings (cross-project limits)
/// Phase 82: Single-row table for cross-project capacity caps
#[async_trait]
pub trait GlobalExecutionSettingsRepository: Send + Sync {
    /// Get global execution settings
    async fn get_settings(&self) -> Result<GlobalExecutionSettings, Box<dyn std::error::Error>>;

    /// Update global execution settings
    /// Enforces max value of 50 for persisted capacity caps
    async fn update_settings(
        &self,
        settings: &GlobalExecutionSettings,
    ) -> Result<GlobalExecutionSettings, Box<dyn std::error::Error>>;
}
