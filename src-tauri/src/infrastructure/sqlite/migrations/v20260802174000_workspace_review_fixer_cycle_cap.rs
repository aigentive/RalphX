//! Migration v20260802174000: persisted Workspace Review fixer cycle cap.

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_fixer_cycle_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "review_settings",
        "workspace_review_fixer_cycle_cap",
        "INTEGER NOT NULL DEFAULT 3",
    )
}
