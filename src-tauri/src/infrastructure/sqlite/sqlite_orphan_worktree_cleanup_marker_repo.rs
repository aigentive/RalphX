use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::entities::ProjectId;
use crate::domain::repositories::{
    OrphanWorktreeCleanupMarker, OrphanWorktreeCleanupMarkerKey,
    OrphanWorktreeCleanupMarkerRepository,
};
use crate::error::AppResult;
use crate::infrastructure::sqlite::DbConnection;

pub struct SqliteOrphanWorktreeCleanupMarkerRepository {
    db: DbConnection,
}

impl SqliteOrphanWorktreeCleanupMarkerRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

#[async_trait]
impl OrphanWorktreeCleanupMarkerRepository for SqliteOrphanWorktreeCleanupMarkerRepository {
    async fn has_recent_marker(
        &self,
        key: &OrphanWorktreeCleanupMarkerKey,
        retry_after: DateTime<Utc>,
    ) -> AppResult<bool> {
        let project_id = key.project_id.as_str().to_string();
        let worktree_path = key.worktree_path.clone();
        let branch_name = key.branch_name.clone();
        let cleanup_status = key.cleanup_status.clone();
        let head_sha = key.head_sha.clone();
        let target_ref = key.target_ref.clone();
        let retry_after = retry_after.to_rfc3339();
        self.db
            .run(move |conn| {
                let exists = conn
                    .query_row(
                        "SELECT 1 FROM orphan_agent_worktree_cleanup_markers
                         WHERE project_id = ?1
                           AND worktree_path = ?2
                           AND branch_name = ?3
                           AND cleanup_status = ?4
                           AND ((head_sha IS NULL AND ?5 IS NULL) OR head_sha = ?5)
                           AND ((target_ref IS NULL AND ?6 IS NULL) OR target_ref = ?6)
                           AND checked_at >= ?7
                         LIMIT 1",
                        rusqlite::params![
                            project_id,
                            worktree_path,
                            branch_name,
                            cleanup_status,
                            head_sha,
                            target_ref,
                            retry_after
                        ],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?
                    .is_some();
                Ok(exists)
            })
            .await
    }

    async fn mark(&self, marker: OrphanWorktreeCleanupMarker) -> AppResult<()> {
        let project_id = marker.key.project_id.as_str().to_string();
        let worktree_path = marker.key.worktree_path;
        let branch_name = marker.key.branch_name;
        let cleanup_status = marker.key.cleanup_status;
        let head_sha = marker.key.head_sha;
        let target_ref = marker.key.target_ref;
        let checked_at = marker.checked_at.to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO orphan_agent_worktree_cleanup_markers (
                        project_id, worktree_path, branch_name, cleanup_status,
                        head_sha, target_ref, checked_at, updated_at
                     )
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                     ON CONFLICT(project_id, worktree_path, branch_name, cleanup_status)
                     DO UPDATE SET
                        head_sha = excluded.head_sha,
                        target_ref = excluded.target_ref,
                        checked_at = excluded.checked_at,
                        updated_at = excluded.updated_at",
                    rusqlite::params![
                        project_id,
                        worktree_path,
                        branch_name,
                        cleanup_status,
                        head_sha,
                        target_ref,
                        checked_at
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn clear_for_worktree(
        &self,
        project_id: &ProjectId,
        worktree_path: &str,
        branch_name: &str,
    ) -> AppResult<()> {
        let project_id = project_id.as_str().to_string();
        let worktree_path = worktree_path.to_string();
        let branch_name = branch_name.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM orphan_agent_worktree_cleanup_markers
                     WHERE project_id = ?1 AND worktree_path = ?2 AND branch_name = ?3",
                    rusqlite::params![project_id, worktree_path, branch_name],
                )?;
                Ok(())
            })
            .await
    }
}
