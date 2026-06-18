// Migration v72: Add cross_project_checked column to ideation_sessions
//
// When set to false (0) on a session that has a plan_artifact_id, the backend
// rejects proposal creation until cross_project_guide has been called.
// DEFAULT 1 = all existing sessions treated as already checked.

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "cross_project_checked",
        "BOOLEAN NOT NULL DEFAULT 1",
    )?;
    tracing::info!("v72: added cross_project_checked column to ideation_sessions");
    Ok(())
}
