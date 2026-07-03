// Migration v20260630123000: Workspace Review policy setting

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "review_settings",
        "require_workspace_review",
        "INTEGER NOT NULL DEFAULT 1",
    )
}
