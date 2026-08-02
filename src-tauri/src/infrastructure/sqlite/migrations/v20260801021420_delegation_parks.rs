// Migration v20260801021420: delegation parks

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS delegation_parks (
            id TEXT PRIMARY KEY,
            parent_conversation_id TEXT NOT NULL,
            parent_agent_run_id TEXT NOT NULL,
            generation INTEGER NOT NULL,
            wake_policy TEXT NOT NULL,
            wake_on_failure INTEGER NOT NULL,
            state TEXT NOT NULL,
            deadline_at TEXT NOT NULL,
            wake_attempts INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_delegation_parks_state_deadline
            ON delegation_parks(state, deadline_at);
        CREATE INDEX IF NOT EXISTS idx_delegation_parks_conversation_state
            ON delegation_parks(parent_conversation_id, state);

        CREATE TABLE IF NOT EXISTS delegation_park_jobs (
            park_id TEXT NOT NULL,
            job_id TEXT NOT NULL,
            delegated_session_id TEXT NOT NULL,
            delegated_agent_run_id TEXT NOT NULL,
            settled_status TEXT,
            PRIMARY KEY (park_id, delegated_agent_run_id)
        );
        CREATE INDEX IF NOT EXISTS idx_delegation_park_jobs_run
            ON delegation_park_jobs(delegated_agent_run_id);",
    )?;
    Ok(())
}
