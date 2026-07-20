// Migration v20260720131416: disable owned-PR automation for Review PR workspaces

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "UPDATE agent_conversation_workspaces
         SET pr_autofix_enabled = 0,
             pr_auto_merge_desired = 0,
             pr_auto_merge_current = NULL,
             auto_publish_enabled = 1,
             auto_publish_initial_pr_enabled = 0,
             auto_publish_paused_pr_autofix_enabled = NULL,
             auto_publish_paused_pr_auto_merge_desired = NULL,
             publication_push_status = NULL,
             pr_supervision_status = NULL,
             pr_supervision_summary = NULL,
             pr_supervision_updated_at = NULL
         WHERE mode = 'review_pr'",
        [],
    )?;
    Ok(())
}
