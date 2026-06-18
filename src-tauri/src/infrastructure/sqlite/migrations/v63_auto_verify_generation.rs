use crate::error::AppResult;
use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    crate::infrastructure::sqlite::migrations::helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "verification_generation",
        "INTEGER NOT NULL DEFAULT 0",
    )
}
