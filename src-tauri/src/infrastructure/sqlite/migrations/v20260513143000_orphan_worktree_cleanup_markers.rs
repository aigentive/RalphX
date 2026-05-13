// Migration v20260513143000: orphan agent worktree cleanup markers

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS orphan_agent_worktree_cleanup_markers (
            project_id TEXT NOT NULL,
            worktree_path TEXT NOT NULL,
            branch_name TEXT NOT NULL,
            cleanup_status TEXT NOT NULL,
            head_sha TEXT NULL,
            target_ref TEXT NULL,
            checked_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY(project_id, worktree_path, branch_name, cleanup_status)
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_orphan_agent_worktree_cleanup_recent
         ON orphan_agent_worktree_cleanup_markers(project_id, cleanup_status, checked_at)",
        [],
    )?;

    Ok(())
}
