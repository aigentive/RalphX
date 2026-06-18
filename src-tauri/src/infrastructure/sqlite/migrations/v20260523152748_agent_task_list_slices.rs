use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute("PRAGMA foreign_keys = OFF", [])
        .map_err(|error| AppError::Database(error.to_string()))?;

    conn.execute_batch(
        "DROP TABLE IF EXISTS agent_task_lists_new;

         CREATE TABLE agent_task_lists_new (
            id TEXT PRIMARY KEY,
            project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
            scope_type TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            list_sequence INTEGER NOT NULL DEFAULT 1,
            name TEXT,
            created_by_agent TEXT,
            next_task_number INTEGER NOT NULL DEFAULT 1,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(scope_type, scope_id, list_sequence)
         );

         INSERT INTO agent_task_lists_new (
            id,
            project_id,
            scope_type,
            scope_id,
            list_sequence,
            name,
            created_by_agent,
            next_task_number,
            created_at,
            updated_at
         )
         SELECT
            id,
            project_id,
            scope_type,
            scope_id,
            1,
            name,
            created_by_agent,
            next_task_number,
            created_at,
            updated_at
         FROM agent_task_lists;

         DROP TABLE agent_task_lists;
         ALTER TABLE agent_task_lists_new RENAME TO agent_task_lists;

         CREATE INDEX IF NOT EXISTS idx_agent_task_lists_scope
            ON agent_task_lists(scope_type, scope_id, list_sequence DESC);",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    conn.execute("PRAGMA foreign_keys = ON", [])
        .map_err(|error| AppError::Database(error.to_string()))?;

    Ok(())
}
