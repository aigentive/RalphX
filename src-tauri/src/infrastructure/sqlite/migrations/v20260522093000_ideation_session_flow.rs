// Migration v20260522093000: Add session_flow to ideation_sessions
//
// Separates the user-facing flow for ideation-family sessions from purpose
// (general vs verifier child) and origin (internal vs external API).

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "session_flow",
        "TEXT NOT NULL DEFAULT 'ideation'",
    )?;

    tracing::info!("v20260522093000: added session_flow column to ideation_sessions");

    Ok(())
}
