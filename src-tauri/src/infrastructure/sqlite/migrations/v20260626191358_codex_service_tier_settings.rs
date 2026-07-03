// Migration v20260626191358: service tier metadata

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::{add_column_if_not_exists, table_exists};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    if table_exists(conn, "agent_provider_settings") {
        add_column_if_not_exists(conn, "agent_provider_settings", "service_tier", "TEXT")?;
    }

    if table_exists(conn, "agent_runs") {
        add_column_if_not_exists(conn, "agent_runs", "service_tier", "TEXT")?;
    }

    Ok(())
}
