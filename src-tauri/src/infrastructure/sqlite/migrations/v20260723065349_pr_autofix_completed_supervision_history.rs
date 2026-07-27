// Migration v20260723065349: pr autofix completed supervision history
//
// Since PR #854, `pr_autofix_completed` publication events are written in the
// same transaction that moves `pr_supervision_status` to `reviewing`,
// `publishing`, or `paused` — never directly to `monitoring`. The
// v20260522090000 publication-event trigger still derived a `monitoring`
// supervision-history row from that step, so every gated PR-fix completion
// produced a contradictory `publication_event` row next to the correct
// `workspace_snapshot` row. Recreate the trigger without that arm; all other
// mappings are preserved verbatim.

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS trg_agent_workspace_state_history_publication_event;

        CREATE TRIGGER trg_agent_workspace_state_history_publication_event
        AFTER INSERT ON agent_conversation_workspace_publication_events
        BEGIN
            INSERT INTO agent_conversation_workspace_state_history (
                id, conversation_id, state_family, from_state, to_state, source,
                source_event_id, created_at
            )
            SELECT lower(hex(randomblob(16))), NEW.conversation_id,
                'publication_pr_status', NULL,
                CASE
                    WHEN NEW.step IN ('pr_merged', 'external_pr_merged') THEN 'merged'
                    WHEN NEW.step IN ('pr_closed', 'external_pr_closed') THEN 'closed'
                    WHEN NEW.step = 'pr_terminal' AND NEW.status IN ('merged', 'closed', 'draft', 'open') THEN NEW.status
                    WHEN NEW.step = 'external_pr_linked' AND NEW.status = 'draft' THEN 'draft'
                    WHEN NEW.step = 'external_pr_linked' AND NEW.status = 'succeeded' THEN 'open'
                END,
                'publication_event', NEW.id, NEW.created_at
            WHERE CASE
                    WHEN NEW.step IN ('pr_merged', 'external_pr_merged') THEN 'merged'
                    WHEN NEW.step IN ('pr_closed', 'external_pr_closed') THEN 'closed'
                    WHEN NEW.step = 'pr_terminal' AND NEW.status IN ('merged', 'closed', 'draft', 'open') THEN NEW.status
                    WHEN NEW.step = 'external_pr_linked' AND NEW.status = 'draft' THEN 'draft'
                    WHEN NEW.step = 'external_pr_linked' AND NEW.status = 'succeeded' THEN 'open'
                END IS NOT NULL;

            INSERT INTO agent_conversation_workspace_state_history (
                id, conversation_id, state_family, from_state, to_state, source,
                source_event_id, created_at
            )
            SELECT lower(hex(randomblob(16))), NEW.conversation_id,
                'publication_push_status', NULL,
                CASE
                    WHEN NEW.step IN ('published', 'pushed', 'external_pr_linked') AND NEW.status = 'succeeded' THEN 'pushed'
                    WHEN NEW.step IN ('updated_from_base', 'base_current', 'repair_resolved') AND NEW.status = 'succeeded' THEN 'refreshed'
                    WHEN NEW.step = 'no_changes' THEN 'no_changes'
                    WHEN NEW.step = 'description_failed' THEN 'description_failed'
                    WHEN NEW.step = 'needs_agent' THEN 'needs_agent'
                    WHEN NEW.step = 'failed' THEN 'failed'
                END,
                'publication_event', NEW.id, NEW.created_at
            WHERE CASE
                    WHEN NEW.step IN ('published', 'pushed', 'external_pr_linked') AND NEW.status = 'succeeded' THEN 'pushed'
                    WHEN NEW.step IN ('updated_from_base', 'base_current', 'repair_resolved') AND NEW.status = 'succeeded' THEN 'refreshed'
                    WHEN NEW.step = 'no_changes' THEN 'no_changes'
                    WHEN NEW.step = 'description_failed' THEN 'description_failed'
                    WHEN NEW.step = 'needs_agent' THEN 'needs_agent'
                    WHEN NEW.step = 'failed' THEN 'failed'
                END IS NOT NULL;

            INSERT INTO agent_conversation_workspace_state_history (
                id, conversation_id, state_family, from_state, to_state, source,
                source_event_id, created_at
            )
            SELECT lower(hex(randomblob(16))), NEW.conversation_id,
                'pr_supervision_status', NULL,
                CASE
                    WHEN NEW.step = 'pr_supervision' AND NEW.status = 'enabled' THEN 'monitoring'
                    WHEN NEW.step = 'pr_supervision' AND NEW.status = 'disabled' THEN 'disabled'
                    WHEN NEW.step = 'pr_autofix' AND NEW.status = 'needs_agent' THEN 'fixing'
                    WHEN NEW.step IN ('repair_requested', 'repair_deferred') THEN 'fixing'
                    WHEN NEW.step = 'repair_sent' AND NEW.status IN ('started', 'succeeded') THEN 'fixing'
                    WHEN NEW.step = 'repair_sent' AND NEW.status = 'failed' THEN 'blocked'
                    WHEN NEW.step IN ('repair_completed', 'repair_resolved', 'pr_supervision_recovered') THEN 'monitoring'
                END,
                'publication_event', NEW.id, NEW.created_at
            WHERE CASE
                    WHEN NEW.step = 'pr_supervision' AND NEW.status = 'enabled' THEN 'monitoring'
                    WHEN NEW.step = 'pr_supervision' AND NEW.status = 'disabled' THEN 'disabled'
                    WHEN NEW.step = 'pr_autofix' AND NEW.status = 'needs_agent' THEN 'fixing'
                    WHEN NEW.step IN ('repair_requested', 'repair_deferred') THEN 'fixing'
                    WHEN NEW.step = 'repair_sent' AND NEW.status IN ('started', 'succeeded') THEN 'fixing'
                    WHEN NEW.step = 'repair_sent' AND NEW.status = 'failed' THEN 'blocked'
                    WHEN NEW.step IN ('repair_completed', 'repair_resolved', 'pr_supervision_recovered') THEN 'monitoring'
                END IS NOT NULL;
        END;",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    Ok(())
}
