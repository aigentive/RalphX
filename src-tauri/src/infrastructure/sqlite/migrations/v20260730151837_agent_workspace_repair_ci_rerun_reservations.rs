// Migration v20260730151837: agent workspace repair ci rerun reservations

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "BEGIN IMMEDIATE;
         ALTER TABLE agent_workspace_repair_attempts
             ADD COLUMN ci_rerun_count INTEGER NOT NULL DEFAULT 0 CHECK (ci_rerun_count >= 0);
         ALTER TABLE agent_workspace_repair_attempts
             ADD COLUMN ci_rerun_fingerprint TEXT;
         COMMIT;",
    )?;
    Ok(())
}
