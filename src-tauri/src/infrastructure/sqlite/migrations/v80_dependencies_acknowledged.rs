// Migration v80: Add dependencies_acknowledged column to ideation_sessions
//
// Adds `dependencies_acknowledged BOOLEAN NOT NULL DEFAULT 0` to ideation_sessions.
// This column is the foundation for the dependency acknowledgment gate:
// agents must explicitly call acknowledge_dependencies before finalize_proposals
// will proceed when cross-proposal dependencies exist.

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "dependencies_acknowledged",
        "BOOLEAN NOT NULL DEFAULT 0",
    )?;

    tracing::info!("v80: added dependencies_acknowledged column to ideation_sessions");

    Ok(())
}
