// Migration v20260611110952: question skip progress

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "pending_questions",
        "allow_skip",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    add_column_if_not_exists(conn, "pending_questions", "batch_index", "INTEGER")?;
    add_column_if_not_exists(conn, "pending_questions", "batch_total", "INTEGER")?;
    add_column_if_not_exists(
        conn,
        "pending_questions",
        "answer_skipped",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}
