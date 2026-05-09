// Memory-based AppStateRepository implementation for testing
// Uses RwLock for thread-safe storage without a real database

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::domain::entities::app_state::{AppSettings, ExecutionHaltMode};
use crate::domain::entities::ProjectId;
use crate::domain::repositories::AppStateRepository;

/// In-memory implementation of AppStateRepository for testing
pub struct MemoryAppStateRepository {
    settings: Arc<RwLock<AppSettings>>,
}

impl Default for MemoryAppStateRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryAppStateRepository {
    /// Create a new empty in-memory app state repository
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(AppSettings::default())),
        }
    }

    /// Create with a specific active project (for tests)
    pub fn with_active_project(project_id: ProjectId) -> Self {
        Self {
            settings: Arc::new(RwLock::new(AppSettings {
                active_project_id: Some(project_id),
                ..AppSettings::default()
            })),
        }
    }
}

#[async_trait]
impl AppStateRepository for MemoryAppStateRepository {
    async fn get(&self) -> Result<AppSettings, Box<dyn std::error::Error>> {
        let settings = self.settings.read().await;
        Ok(settings.clone())
    }

    async fn set_active_project(
        &self,
        project_id: Option<&ProjectId>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut settings = self.settings.write().await;
        settings.active_project_id = project_id.cloned();
        Ok(())
    }

    async fn set_execution_halt_mode(
        &self,
        halt_mode: ExecutionHaltMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut settings = self.settings.write().await;
        settings.execution_halt_mode = halt_mode;
        Ok(())
    }

    async fn set_last_seen_release_notes_version(
        &self,
        version: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut settings = self.settings.write().await;
        settings.last_seen_release_notes_version = version.map(str::to_string);
        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_app_state_repo_tests.rs"]
mod tests;
