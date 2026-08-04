use tokio::sync::RwLock;

use crate::domain::entities::ProjectId;

/// Tracks the currently active project for execution scoping.
/// Commands without explicit project_id use the active project.
/// Phase 90: Simple RwLock — DB persistence eliminates the startup race condition.
pub struct ActiveProjectState {
    /// The currently active project, if any
    current: RwLock<Option<ProjectId>>,
}

impl std::fmt::Debug for ActiveProjectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveProjectState")
            .field("current", &self.current)
            .finish()
    }
}

impl Default for ActiveProjectState {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveProjectState {
    /// Create a new ActiveProjectState with no active project
    pub fn new() -> Self {
        Self {
            current: RwLock::new(None),
        }
    }

    /// Get the current active project ID
    pub async fn get(&self) -> Option<ProjectId> {
        self.current.read().await.clone()
    }

    /// Set the active project
    pub async fn set(&self, project_id: Option<ProjectId>) {
        *self.current.write().await = project_id;
    }

    /// Set the active project only when it changed.
    pub async fn set_if_changed(&self, project_id: Option<ProjectId>) -> bool {
        let mut current = self.current.write().await;
        if *current == project_id {
            return false;
        }
        *current = project_id;
        true
    }
}
