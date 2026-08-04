// Migration v20260804120000: durable observed base tip for completed direct PR freshness routes

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_repair_attempts",
        "base_update_target_commit",
        "TEXT",
    )
}
