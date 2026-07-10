// Migration v20260710003315: execution plan halt mode

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    super::helpers::add_column_if_not_exists(
        conn,
        "execution_plans",
        "halt_mode",
        "TEXT NOT NULL DEFAULT 'running' CHECK (halt_mode IN ('running', 'paused', 'stopped'))",
    )
}
