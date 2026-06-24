// Migration v20260327233752: Add pending_initial_prompt column to ideation_sessions
//
// Adds `pending_initial_prompt TEXT NULL` to ideation_sessions.
// This column stores the initial prompt for sessions that failed to spawn due to
// ideation capacity limits. The drain service reads this column to auto-launch
// deferred sessions when capacity becomes available.

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "ideation_sessions", "pending_initial_prompt", "TEXT")?;

    tracing::info!("v20260327233752: added pending_initial_prompt column to ideation_sessions");

    Ok(())
}
