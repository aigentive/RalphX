// Migration v20260731023949: agent run identity

use rusqlite::Connection;

use crate::{
    error::AppResult, infrastructure::sqlite::migrations::helpers::add_column_if_not_exists,
};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(conn, "agent_runs", "agent_name", "TEXT")?;
    add_column_if_not_exists(conn, "agent_runs", "launch_role", "TEXT")?;
    add_column_if_not_exists(conn, "agent_runs", "runtime_source", "TEXT")?;
    Ok(())
}
