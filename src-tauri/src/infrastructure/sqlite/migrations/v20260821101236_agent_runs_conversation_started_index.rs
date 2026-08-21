// Migration v20260821101236: agent runs conversation started index
//
// The sidebar listing resolves the latest run for every visible conversation in one batched
// `ROW_NUMBER() OVER (PARTITION BY conversation_id ORDER BY started_at DESC)` pass.
// `idx_agent_runs_conversation` covers only the equality lookup, leaving SQLite to sort each
// partition. This composite index makes the partitioned scan index-ordered.

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_runs_conversation_started
         ON agent_runs(conversation_id, started_at DESC)",
        [],
    )?;
    Ok(())
}
