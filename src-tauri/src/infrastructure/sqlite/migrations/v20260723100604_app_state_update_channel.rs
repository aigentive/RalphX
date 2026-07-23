// Migration v20260723100604: persist app update channel

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "app_state",
        "update_channel",
        "TEXT NOT NULL DEFAULT 'stable'",
    )?;
    Ok(())
}
