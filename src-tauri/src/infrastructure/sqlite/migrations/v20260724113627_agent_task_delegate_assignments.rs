// Migration v20260724113627: agent task delegate assignments

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_task_delegate_assignments (
            id TEXT PRIMARY KEY,
            delegated_session_id TEXT NOT NULL
                REFERENCES delegated_sessions(id) ON DELETE CASCADE,
            attempt_number INTEGER NOT NULL CHECK (attempt_number > 0),
            caller_agent_run_id TEXT NOT NULL
                REFERENCES agent_runs(id) ON DELETE RESTRICT,
            delegated_agent_run_id TEXT
                REFERENCES agent_runs(id) ON DELETE RESTRICT,
            task_list_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            delegate_agent_name TEXT NOT NULL,
            state TEXT NOT NULL CHECK (
                state IN (
                    'reserved',
                    'active',
                    'completion_requested',
                    'release_requested',
                    'completed',
                    'released',
                    'failed',
                    'cancelled'
                )
            ),
            prior_owner_agent TEXT,
            settlement_reason TEXT,
            completion_metadata_json TEXT,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            run_bound_at DATETIME,
            settled_at DATETIME,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(delegated_session_id, attempt_number),
            FOREIGN KEY (task_list_id, task_id)
                REFERENCES agent_tasks(task_list_id, id) ON DELETE RESTRICT
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_task_assignments_unresolved_session
            ON agent_task_delegate_assignments(delegated_session_id)
            WHERE state IN (
                'reserved',
                'active',
                'completion_requested',
                'release_requested'
            );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_task_assignments_unresolved_task
            ON agent_task_delegate_assignments(task_list_id, task_id)
            WHERE state IN (
                'reserved',
                'active',
                'completion_requested',
                'release_requested'
            );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_task_assignments_delegated_run
            ON agent_task_delegate_assignments(delegated_agent_run_id)
            WHERE delegated_agent_run_id IS NOT NULL;

        CREATE INDEX IF NOT EXISTS idx_agent_task_assignments_task_history
            ON agent_task_delegate_assignments(task_list_id, task_id, attempt_number DESC);",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    Ok(())
}
