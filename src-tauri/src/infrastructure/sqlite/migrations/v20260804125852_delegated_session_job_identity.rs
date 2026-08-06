// Migration v20260804125852: delegated session job identity

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "delegated_sessions", "job_id", "TEXT")?;
    helpers::add_column_if_not_exists(conn, "delegated_sessions", "parent_agent_run_id", "TEXT")
}
