// Migration v66: Add source_project_id and source_session_id columns to ideation_sessions
//
// Supports cross-project plan import: tracks the origin project and session
// when a plan is imported from another project on the same RalphX instance.

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "ideation_sessions", "source_project_id", "TEXT")?;

    helpers::add_column_if_not_exists(conn, "ideation_sessions", "source_session_id", "TEXT")?;

    tracing::info!(
        "v66: added source_project_id and source_session_id columns to ideation_sessions"
    );

    Ok(())
}
