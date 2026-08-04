// Migration v20260803113302: agent workspace publish lease

use rusqlite::Connection;

use super::helpers::add_column_if_not_exists;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "publish_lease_owner_run_id",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "publish_lease_token",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "publish_lease_heartbeat_at",
        "TEXT NULL",
    )?;
    Ok(())
}
