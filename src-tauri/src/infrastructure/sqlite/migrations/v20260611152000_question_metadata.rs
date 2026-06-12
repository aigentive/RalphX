// Migration v20260611152000: pending question metadata

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(conn, "pending_questions", "metadata", "TEXT")?;
    Ok(())
}
