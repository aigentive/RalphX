// Migration v20260723170404: agent workspace publication pushed sha

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "publication_pushed_sha",
        "TEXT",
    )
}
