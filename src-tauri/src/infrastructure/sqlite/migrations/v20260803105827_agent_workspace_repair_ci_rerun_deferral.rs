// Migration v20260803105827: agent workspace repair ci rerun deferral

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "BEGIN IMMEDIATE;
         ALTER TABLE agent_workspace_repair_attempts
             ADD COLUMN ci_rerun_pending_run_id INTEGER;
         ALTER TABLE agent_workspace_repair_attempts
             ADD COLUMN ci_rerun_deferred_since TEXT;
         COMMIT;",
    )?;
    Ok(())
}
