// Migration v20260720102513: add tasks feature state

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "ideation_settings",
        "tasks_feature_state",
        "TEXT NOT NULL DEFAULT 'disabled'
         CHECK (tasks_feature_state IN ('enabled', 'draining', 'disabled'))",
    )?;
    conn.execute(
        "UPDATE ideation_settings
         SET tasks_feature_state = 'enabled'
         WHERE tasks_enabled = 1 AND tasks_feature_state = 'disabled'",
        [],
    )?;
    Ok(())
}
