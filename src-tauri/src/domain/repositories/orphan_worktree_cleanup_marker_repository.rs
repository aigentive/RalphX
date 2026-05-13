use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::ProjectId;
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrphanWorktreeCleanupMarkerKey {
    pub project_id: ProjectId,
    pub worktree_path: String,
    pub branch_name: String,
    pub cleanup_status: String,
    pub head_sha: Option<String>,
    pub target_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanWorktreeCleanupMarker {
    pub key: OrphanWorktreeCleanupMarkerKey,
    pub checked_at: DateTime<Utc>,
}

#[async_trait]
pub trait OrphanWorktreeCleanupMarkerRepository: Send + Sync {
    async fn has_recent_marker(
        &self,
        key: &OrphanWorktreeCleanupMarkerKey,
        retry_after: DateTime<Utc>,
    ) -> AppResult<bool>;

    async fn mark(&self, marker: OrphanWorktreeCleanupMarker) -> AppResult<()>;

    async fn clear_for_worktree(
        &self,
        project_id: &ProjectId,
        worktree_path: &str,
        branch_name: &str,
    ) -> AppResult<()>;
}
