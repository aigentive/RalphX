// Migration v20260406043151: add last_effective_model column to ideation_sessions

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "ALTER TABLE ideation_sessions ADD COLUMN last_effective_model TEXT DEFAULT NULL;",
    )
    .map_err(|e| {
        crate::error::AppError::Database(format!("Migration v20260406043151 failed: {}", e))
    })?;
    Ok(())
}
