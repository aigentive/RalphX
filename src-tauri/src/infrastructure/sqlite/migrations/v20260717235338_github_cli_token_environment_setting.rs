// Migration v20260717235338: github cli token environment setting

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "app_state",
        "remove_inherited_github_cli_tokens",
        "INTEGER NOT NULL DEFAULT 1 CHECK (remove_inherited_github_cli_tokens IN (0, 1))",
    )?;
    Ok(())
}
