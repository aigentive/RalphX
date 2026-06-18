// Migration v20260406043153: add model column to running_agents

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("ALTER TABLE running_agents ADD COLUMN model TEXT DEFAULT NULL;")
        .map_err(|e| {
            crate::error::AppError::Database(format!("Migration v20260406043153 failed: {}", e))
        })?;
    Ok(())
}
