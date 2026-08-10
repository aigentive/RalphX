use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS project_repository_capability (
            project_id TEXT PRIMARY KEY NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            kind TEXT NOT NULL,
            fetch_url TEXT,
            push_url TEXT,
            message TEXT,
            inspected_at TEXT NOT NULL,
            working_directory TEXT NOT NULL
        );",
    )?;
    Ok(())
}
