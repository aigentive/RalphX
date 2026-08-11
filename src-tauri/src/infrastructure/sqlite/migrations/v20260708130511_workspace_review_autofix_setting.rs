// Migration v20260708130511: workspace review autofix setting

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "review_settings",
        "autofix_workspace_review_blocking_findings",
        "INTEGER NOT NULL DEFAULT 1",
    )
}
