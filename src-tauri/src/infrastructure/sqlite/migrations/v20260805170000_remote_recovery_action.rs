use crate::error::AppResult;
use rusqlite::Connection;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "remote_resume_requests", "recovery_action", "TEXT")?;
    Ok(())
}
