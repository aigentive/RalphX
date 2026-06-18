// Migration v71: Add target_project column to proposals table
//
// Stores an optional target project identifier on a proposal.
// When set, the proposal is associated with a specific project target.

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "task_proposals", "target_project", "TEXT")?;

    tracing::info!("v71: added target_project column to task_proposals");

    Ok(())
}
