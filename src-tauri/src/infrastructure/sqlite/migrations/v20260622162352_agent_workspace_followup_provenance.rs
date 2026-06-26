// Migration v20260622162352: agent workspace followup provenance

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    for (column, definition) in [
        ("followup_origin_conversation_id", "TEXT NULL"),
        ("followup_source_task_id", "TEXT NULL"),
        ("followup_source_context_type", "TEXT NULL"),
        ("followup_source_context_id", "TEXT NULL"),
        ("followup_source_agent_name", "TEXT NULL"),
        ("followup_spawn_reason", "TEXT NULL"),
        ("followup_blocker_fingerprint", "TEXT NULL"),
    ] {
        helpers::add_column_if_not_exists(
            conn,
            "agent_conversation_workspaces",
            column,
            definition,
        )?;
    }

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_workspaces_followup_blocker
         ON agent_conversation_workspaces(
            followup_origin_conversation_id,
            followup_source_task_id,
            followup_blocker_fingerprint,
            status,
            updated_at
         )",
        [],
    )?;

    Ok(())
}
