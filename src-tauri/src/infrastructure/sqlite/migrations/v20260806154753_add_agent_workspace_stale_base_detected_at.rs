// Migration v20260806154753: add agent workspace stale base detected at

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "stale_base_detected_at",
        "TEXT",
    )
}
