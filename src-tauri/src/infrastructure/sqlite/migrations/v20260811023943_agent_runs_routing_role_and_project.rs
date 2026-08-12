// Migration v20260811023943: agent runs routing role and project
//
// Persists the authoritative routing role and project id resolved at spawn
// time so per-request authorization (Atlassian MCP tiers) has real authority.
// `launch_role` remains display attribution covering only three agents and
// must not be used for authorization.
//
// Both columns are nullable and forward-only. Runs started before this
// migration read as NULL, which the authorization path treats as "deny".

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(conn, "agent_runs", "routing_role", "TEXT")?;
    add_column_if_not_exists(conn, "agent_runs", "project_id", "TEXT")?;
    Ok(())
}
