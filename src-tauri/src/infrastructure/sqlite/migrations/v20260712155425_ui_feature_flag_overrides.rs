use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ui_feature_flag_overrides (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            agent_personas INTEGER NULL
        )",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    conn.execute(
        "INSERT OR IGNORE INTO ui_feature_flag_overrides (id, agent_personas) VALUES (1, NULL)",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    Ok(())
}
