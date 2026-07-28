// Migration v20260727115607: agent workspace publication pushed sha

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "publication_pushed_sha",
        "TEXT",
    )?;
    Ok(())
}
