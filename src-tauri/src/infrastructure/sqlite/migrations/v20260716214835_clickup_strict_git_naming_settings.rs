// Migration v20260716214835: clickup strict git naming settings

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "clickup_integration_settings",
        "strict_git_naming_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_not_exists(
        conn,
        "clickup_integration_settings",
        "branch_name_template",
        "TEXT NOT NULL DEFAULT ':taskId:_:taskName:_:username:'",
    )?;
    add_column_if_not_exists(
        conn,
        "clickup_integration_settings",
        "commit_subject_template",
        "TEXT NOT NULL DEFAULT ':taskId: - :taskName:'",
    )?;
    add_column_if_not_exists(
        conn,
        "clickup_integration_settings",
        "pr_title_template",
        "TEXT NOT NULL DEFAULT ':taskId: - :taskName:'",
    )?;
    Ok(())
}
