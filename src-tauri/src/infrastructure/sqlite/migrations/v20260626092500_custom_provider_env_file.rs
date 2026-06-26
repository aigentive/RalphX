// Migration v20260626092500: custom provider env file settings

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
        "custom_env_file_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_provider_settings",
        "custom_env_file_path",
        "TEXT",
    )?;

    Ok(())
}
