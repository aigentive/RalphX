// Migration v20260810142632: agent workspace repair narrative fields

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_repair_attempts",
        "what_happened",
        "TEXT",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_repair_attempts",
        "what_i_did",
        "TEXT",
    )
}
