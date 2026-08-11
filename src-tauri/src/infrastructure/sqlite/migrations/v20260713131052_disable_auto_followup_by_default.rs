// Migration v20260713131052: disable auto followup by default

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "UPDATE review_settings
         SET auto_create_followup_agent_conversation = 0
         WHERE id = 1",
        [],
    )?;
    Ok(())
}
