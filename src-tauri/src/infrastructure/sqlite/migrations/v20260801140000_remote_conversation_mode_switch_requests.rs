// Migration v20260801140000: remote conversation MODE SWITCH requests (WP5a)
//
// Forward-only, numbered AFTER v20260801130000 (the continuation-intent table). This is the
// fourth remote intent surface and it gets its own table for the same reason the first three
// did: the dispatcher must be able to prove which terminal host call a claimed row authorizes.
// A shared table with a `kind` column would let a row claimed by the mode-switch loop be
// indistinguishable from one claimed by the send loop, and these two loops reach very different
// authority — `switch_agent_conversation_mode_for_state` prepares worktrees and can cross the
// plan/review boundary, `send_message` mints a turn.

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_conversation_mode_switch_requests (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            target_mode TEXT NOT NULL,
            status TEXT NOT NULL,
            error_code TEXT,
            requested_by_device_id TEXT NOT NULL,
            claimed_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_conversation_mode_switch_requests_status
            ON remote_conversation_mode_switch_requests(status);
        CREATE INDEX IF NOT EXISTS idx_remote_conversation_mode_switch_requests_conversation
            ON remote_conversation_mode_switch_requests(conversation_id);",
    )?;
    Ok(())
}
