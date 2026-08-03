// Migration v20260731125157: add workspace repair fingerprint state
//
// A PR autofix repair attempt can park itself against an exact failure fingerprint, but that hold
// dies with the attempt. Once a streak settles blocked-exhausted, the next poll starts a brand new
// streak with no memory of what already failed, which is how one unchanged failing check consumed
// four agent generations on 2026-07-31. Persisting the last blocked fingerprint on the workspace
// gives the poller cross-streak memory that outlives any single attempt.

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "last_blocked_pr_health_fingerprint",
        "TEXT",
    )?;

    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "last_blocked_pr_health_at",
        "TEXT",
    )?;

    tracing::info!("Migration v20260731125157: workspace repair fingerprint state ready");

    Ok(())
}
