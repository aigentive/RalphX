// Migration v58: Add composite index for metrics cycle time queries
//
// Adds idx_task_state_history_task_created on task_state_history(task_id, created_at).
// This composite index optimises the LAG() window function query used to compute
// cycle time breakdowns in the upcoming get_project_stats command.
//
// The existing idx_task_state_history_task_id covers single-column task_id lookups;
// the new composite index extends that to also cover the ORDER BY created_at clause
// needed for window functions — eliminating a full table scan per task group.
//
// Safe: CREATE INDEX IF NOT EXISTS is idempotent. No data migration needed.

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::create_index_if_not_exists(
        conn,
        "idx_task_state_history_task_created",
        "task_state_history",
        "task_id, created_at",
    )?;

    tracing::info!("v58: created idx_task_state_history_task_created on task_state_history(task_id, created_at)");

    Ok(())
}
