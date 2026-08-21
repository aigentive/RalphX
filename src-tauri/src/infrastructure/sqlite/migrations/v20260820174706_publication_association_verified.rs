// Migration v20260820174706: publication association verified
//
// Adds the `publication_association_verified_at` marker used by external-PR
// reconciliation to stop re-verifying workspaces whose PR is already terminal
// and whose PR-to-branch association was confirmed once, then cleans up the
// duplicate terminal publication events those repeated passes appended.

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "publication_association_verified_at",
        "TEXT NULL",
    )?;

    // Existing terminal rows are treated as verified: the linked reconciliation
    // path has been re-confirming their PR head against the workspace branch on
    // every trigger, so persisted terminal state is the accumulated evidence.
    conn.execute(
        "UPDATE agent_conversation_workspaces
            SET publication_association_verified_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
            WHERE publication_pr_status IN ('merged', 'closed')
              AND publication_pr_number IS NOT NULL
              AND publication_association_verified_at IS NULL",
        [],
    )?;

    // Drop the replayed terminal events, keeping the earliest per
    // (conversation_id, step). The correlated EXISTS seeks on the existing
    // (conversation_id, created_at) index and short-circuits on the first
    // earlier row; `id` breaks created_at ties deterministically.
    conn.execute(
        "DELETE FROM agent_conversation_workspace_publication_events AS o
            WHERE o.step IN ('pr_merged', 'pr_closed')
              AND EXISTS (
                SELECT 1
                  FROM agent_conversation_workspace_publication_events AS e
                 WHERE e.conversation_id = o.conversation_id
                   AND e.step = o.step
                   AND (e.created_at < o.created_at
                        OR (e.created_at = o.created_at AND e.id < o.id))
              )",
        [],
    )?;

    Ok(())
}
