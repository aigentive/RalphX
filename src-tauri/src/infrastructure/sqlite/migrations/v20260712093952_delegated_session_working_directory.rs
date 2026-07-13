// Migration v20260712093952: delegated session working directory

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("ALTER TABLE delegated_sessions ADD COLUMN working_directory TEXT;")
        .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(())
}
