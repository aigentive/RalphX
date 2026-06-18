// Migration v20260612124826: provider cli management policy

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::{add_column_if_not_exists, table_exists};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    if !table_exists(conn, "agent_provider_settings") {
        return Ok(());
    }

    add_column_if_not_exists(
        conn,
        "agent_provider_settings",
        "cli_management_mode",
        "TEXT NOT NULL DEFAULT 'user_managed'",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_provider_settings",
        "auto_update_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    )?;

    Ok(())
}
