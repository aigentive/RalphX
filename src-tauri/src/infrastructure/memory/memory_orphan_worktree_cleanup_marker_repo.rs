use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use crate::domain::entities::ProjectId;
use crate::domain::repositories::{
    OrphanWorktreeCleanupMarker, OrphanWorktreeCleanupMarkerKey,
    OrphanWorktreeCleanupMarkerRepository,
};
use crate::error::AppResult;

pub struct MemoryOrphanWorktreeCleanupMarkerRepository {
    markers: RwLock<HashMap<OrphanWorktreeCleanupMarkerKey, OrphanWorktreeCleanupMarker>>,
}

impl MemoryOrphanWorktreeCleanupMarkerRepository {
    pub fn new() -> Self {
        Self {
            markers: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryOrphanWorktreeCleanupMarkerRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OrphanWorktreeCleanupMarkerRepository for MemoryOrphanWorktreeCleanupMarkerRepository {
    async fn has_recent_marker(
        &self,
        key: &OrphanWorktreeCleanupMarkerKey,
        retry_after: DateTime<Utc>,
    ) -> AppResult<bool> {
        Ok(self
            .markers
            .read()
            .await
            .get(key)
            .is_some_and(|marker| marker.checked_at >= retry_after))
    }

    async fn mark(&self, marker: OrphanWorktreeCleanupMarker) -> AppResult<()> {
        self.markers
            .write()
            .await
            .insert(marker.key.clone(), marker);
        Ok(())
    }

    async fn clear_for_worktree(
        &self,
        project_id: &ProjectId,
        worktree_path: &str,
        branch_name: &str,
    ) -> AppResult<()> {
        self.markers.write().await.retain(|key, _| {
            key.project_id != *project_id
                || key.worktree_path != worktree_path
                || key.branch_name != branch_name
        });
        Ok(())
    }
}
