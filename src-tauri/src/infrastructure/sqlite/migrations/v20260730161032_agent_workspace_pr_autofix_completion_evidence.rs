// Migration v20260730161032: agent workspace pr autofix completion evidence

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "BEGIN;
         ALTER TABLE agent_workspace_repair_attempts
             ADD COLUMN pr_autofix_dispatch_head_commit TEXT;
         ALTER TABLE agent_workspace_repair_attempts
             ADD COLUMN pr_autofix_health_fingerprint TEXT;
         COMMIT;",
    )?;
    Ok(())
}
