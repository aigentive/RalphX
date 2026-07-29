use rusqlite::Connection;

use crate::{
    error::AppResult, infrastructure::sqlite::migrations::helpers::add_column_if_not_exists,
};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    for (column, definition) in [
        ("current_plan_context_fingerprint", "TEXT NULL"),
        ("reviewed_plan_context_fingerprint", "TEXT NULL"),
    ] {
        add_column_if_not_exists(conn, "agent_workspace_review_monitors", column, definition)?;
    }
    Ok(())
}
