// Migration v77: Add auto-accept gating columns to ideation_sessions
//
// Adds three columns to support the expected_proposal_count auto-accept gating feature:
// - expected_proposal_count: how many proposals the session expects before auto-accepting
// - auto_accept_status: current status of the auto-accept gate (e.g., 'pending', 'triggered')
// - auto_accept_started_at: timestamp when auto-accept gating was activated

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "expected_proposal_count",
        "INTEGER",
    )?;

    helpers::add_column_if_not_exists(conn, "ideation_sessions", "auto_accept_status", "TEXT")?;

    helpers::add_column_if_not_exists(conn, "ideation_sessions", "auto_accept_started_at", "TEXT")?;

    tracing::info!(
        "v77: added expected_proposal_count, auto_accept_status, auto_accept_started_at columns to ideation_sessions"
    );

    Ok(())
}
