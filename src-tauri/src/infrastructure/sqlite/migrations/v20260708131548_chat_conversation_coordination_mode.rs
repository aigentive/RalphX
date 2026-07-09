// Migration v20260708131548: chat conversation coordination mode

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "chat_conversations",
        "coordination_mode",
        "TEXT NOT NULL DEFAULT 'solo' CHECK(coordination_mode IN ('solo', 'legacy_claude_team', 'rx_native_team'))",
    )?;
    Ok(())
}
