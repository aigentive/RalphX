// Migration v68: Add session_purpose column to ideation_sessions
//
// Supports purpose-based routing: distinguishes between "general" ideation sessions
// and "verification" child sessions spawned by the ralphx-plan-verifier agent.

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "session_purpose",
        "TEXT NOT NULL DEFAULT 'general'",
    )?;

    tracing::info!("v68: added session_purpose column to ideation_sessions");

    Ok(())
}
