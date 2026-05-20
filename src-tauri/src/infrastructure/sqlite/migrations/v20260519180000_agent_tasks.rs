use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_task_lists (
            id TEXT PRIMARY KEY,
            project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
            scope_type TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            name TEXT,
            created_by_agent TEXT,
            next_task_number INTEGER NOT NULL DEFAULT 1,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(scope_type, scope_id)
        );

        CREATE TABLE IF NOT EXISTS agent_tasks (
            id TEXT NOT NULL,
            task_list_id TEXT NOT NULL REFERENCES agent_task_lists(id) ON DELETE CASCADE,
            task_number INTEGER NOT NULL,
            title TEXT NOT NULL,
            details TEXT NOT NULL,
            active_label TEXT,
            owner_agent TEXT,
            state TEXT NOT NULL CHECK (state IN ('open', 'active', 'done', 'dropped')),
            metadata_json TEXT,
            version INTEGER NOT NULL DEFAULT 1,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            completed_at DATETIME,
            PRIMARY KEY (task_list_id, id),
            UNIQUE(task_list_id, task_number)
        );

        CREATE TABLE IF NOT EXISTS agent_task_dependencies (
            task_list_id TEXT NOT NULL,
            blocker_task_id TEXT NOT NULL,
            blocked_task_id TEXT NOT NULL,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (task_list_id, blocker_task_id, blocked_task_id),
            CHECK(blocker_task_id != blocked_task_id),
            FOREIGN KEY (task_list_id, blocker_task_id)
                REFERENCES agent_tasks(task_list_id, id) ON DELETE CASCADE,
            FOREIGN KEY (task_list_id, blocked_task_id)
                REFERENCES agent_tasks(task_list_id, id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS agent_task_events (
            event_id TEXT PRIMARY KEY,
            task_list_id TEXT NOT NULL REFERENCES agent_task_lists(id) ON DELETE CASCADE,
            seq INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            actor_agent TEXT,
            task_id TEXT,
            payload_json TEXT NOT NULL,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(task_list_id, seq)
        );

        CREATE INDEX IF NOT EXISTS idx_agent_task_lists_scope
            ON agent_task_lists(scope_type, scope_id);

        CREATE INDEX IF NOT EXISTS idx_agent_tasks_list_state
            ON agent_tasks(task_list_id, state, updated_at DESC);

        CREATE INDEX IF NOT EXISTS idx_agent_task_dependencies_blocked
            ON agent_task_dependencies(task_list_id, blocked_task_id);

        CREATE INDEX IF NOT EXISTS idx_agent_task_events_list_seq
            ON agent_task_events(task_list_id, seq);",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    Ok(())
}
