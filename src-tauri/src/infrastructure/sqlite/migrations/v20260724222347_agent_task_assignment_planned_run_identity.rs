// Migration v20260724222347: agent task assignment planned run identity

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_task_delegate_assignments",
        "planned_delegated_agent_run_id",
        "TEXT",
    )?;
    conn.execute_batch(
        "UPDATE agent_task_delegate_assignments
         SET planned_delegated_agent_run_id = delegated_agent_run_id,
             delegated_agent_run_id = NULL
         WHERE state = 'reserved'
           AND planned_delegated_agent_run_id IS NULL
           AND delegated_agent_run_id IS NOT NULL;

         UPDATE agent_task_delegate_assignments
         SET planned_delegated_agent_run_id = delegated_agent_run_id
         WHERE state != 'reserved'
           AND planned_delegated_agent_run_id IS NULL
           AND delegated_agent_run_id IS NOT NULL;

         CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_task_assignments_planned_run
            ON agent_task_delegate_assignments(planned_delegated_agent_run_id)
            WHERE planned_delegated_agent_run_id IS NOT NULL;",
    )?;
    Ok(())
}
