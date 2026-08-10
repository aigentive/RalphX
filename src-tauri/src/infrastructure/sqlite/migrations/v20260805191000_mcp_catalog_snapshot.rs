use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mcp_catalog_snapshot (
            scope_project_id TEXT NULL,
            provider TEXT NOT NULL,
            response_json TEXT NOT NULL,
            captured_at TEXT NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_catalog_snapshot_global
            ON mcp_catalog_snapshot(provider)
            WHERE scope_project_id IS NULL;
         CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_catalog_snapshot_project
            ON mcp_catalog_snapshot(scope_project_id, provider)
            WHERE scope_project_id IS NOT NULL;",
    )?;
    Ok(())
}
