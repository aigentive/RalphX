// Migration v20260731170447: agent workspace repair runtime conversation
//
// Fixer agents (workspace repair and PR autofix) used to run inside the parent workspace
// conversation, which made follow-up user messages resolve to project chat instead of the fixer.
// Each repair attempt now owns a dedicated child conversation. Recording that child on the attempt
// is what lets completion authority map a child runtime back to the workspace that owns it without
// trusting `parent_conversation_id`.
//
// NULL is the durable marker for a legacy, parent-hosted attempt. There is deliberately no backfill.

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_repair_attempts",
        "runtime_conversation_id",
        "TEXT NULL",
    )?;

    helpers::create_index_if_not_exists(
        conn,
        "idx_agent_workspace_repair_attempts_runtime_conversation",
        "agent_workspace_repair_attempts",
        "runtime_conversation_id",
    )?;

    tracing::info!("Migration v20260731170447: repair attempt runtime conversation ready");

    Ok(())
}
