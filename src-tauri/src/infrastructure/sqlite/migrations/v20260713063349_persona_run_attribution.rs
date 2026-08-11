// Migration v20260713063349: persona run attribution

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    for (column, ty) in [
        ("persona_id", "TEXT"),
        ("persona_slug", "TEXT"),
        ("persona_version", "INTEGER"),
        ("persona_content_hash", "TEXT"),
        ("persona_injected", "INTEGER"),
        ("persona_skipped_reason", "TEXT"),
    ] {
        add_column_if_not_exists(conn, "agent_runs", column, ty)?;
    }

    Ok(())
}
