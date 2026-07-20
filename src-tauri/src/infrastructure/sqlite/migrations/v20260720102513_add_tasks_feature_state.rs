// Migration v20260720102513: add tasks feature state

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "ALTER TABLE ideation_settings
         ADD COLUMN tasks_feature_state TEXT NOT NULL DEFAULT 'disabled'
         CHECK (tasks_feature_state IN ('enabled', 'draining', 'disabled'))",
        [],
    )?;
    conn.execute(
        "UPDATE ideation_settings
         SET tasks_feature_state = CASE WHEN tasks_enabled = 1 THEN 'enabled' ELSE 'disabled' END",
        [],
    )?;
    Ok(())
}
