use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_conversation_workspace_state_history (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            state_family TEXT NOT NULL CHECK (
                state_family IN (
                    'workspace_status',
                    'publication_pr_status',
                    'publication_push_status',
                    'pr_supervision_status'
                )
            ),
            from_state TEXT NULL,
            to_state TEXT NOT NULL,
            source TEXT NOT NULL,
            source_event_id TEXT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(conversation_id)
                REFERENCES agent_conversation_workspaces(conversation_id)
                ON DELETE CASCADE,
            FOREIGN KEY(source_event_id)
                REFERENCES agent_conversation_workspace_publication_events(id)
                ON DELETE SET NULL
        );

        CREATE INDEX IF NOT EXISTS idx_agent_workspace_state_history_conversation
            ON agent_conversation_workspace_state_history(conversation_id, state_family, created_at);

        CREATE INDEX IF NOT EXISTS idx_agent_workspace_state_history_family_state
            ON agent_conversation_workspace_state_history(state_family, to_state, created_at);

        INSERT OR IGNORE INTO agent_conversation_workspace_state_history (
            id, conversation_id, state_family, from_state, to_state, source,
            source_event_id, created_at
        )
        SELECT
            'snapshot:' || conversation_id || ':workspace_status',
            conversation_id,
            'workspace_status',
            NULL,
            status,
            'workspace_snapshot_backfill',
            NULL,
            created_at
        FROM agent_conversation_workspaces
        WHERE status IS NOT NULL;

        INSERT OR IGNORE INTO agent_conversation_workspace_state_history (
            id, conversation_id, state_family, from_state, to_state, source,
            source_event_id, created_at
        )
        SELECT
            'snapshot:' || conversation_id || ':publication_pr_status',
            conversation_id,
            'publication_pr_status',
            NULL,
            publication_pr_status,
            'workspace_snapshot_backfill',
            NULL,
            updated_at
        FROM agent_conversation_workspaces
        WHERE publication_pr_status IS NOT NULL;

        INSERT OR IGNORE INTO agent_conversation_workspace_state_history (
            id, conversation_id, state_family, from_state, to_state, source,
            source_event_id, created_at
        )
        SELECT
            'snapshot:' || conversation_id || ':publication_push_status',
            conversation_id,
            'publication_push_status',
            NULL,
            publication_push_status,
            'workspace_snapshot_backfill',
            NULL,
            updated_at
        FROM agent_conversation_workspaces
        WHERE publication_push_status IS NOT NULL;

        INSERT OR IGNORE INTO agent_conversation_workspace_state_history (
            id, conversation_id, state_family, from_state, to_state, source,
            source_event_id, created_at
        )
        SELECT
            'snapshot:' || conversation_id || ':pr_supervision_status',
            conversation_id,
            'pr_supervision_status',
            NULL,
            pr_supervision_status,
            'workspace_snapshot_backfill',
            NULL,
            COALESCE(pr_supervision_updated_at, updated_at)
        FROM agent_conversation_workspaces
        WHERE pr_supervision_status IS NOT NULL;

        INSERT OR IGNORE INTO agent_conversation_workspace_state_history (
            id, conversation_id, state_family, from_state, to_state, source,
            source_event_id, created_at
        )
        SELECT
            'event:' || id || ':publication_pr_status',
            conversation_id,
            'publication_pr_status',
            NULL,
            CASE
                WHEN step IN ('pr_merged', 'external_pr_merged') THEN 'merged'
                WHEN step IN ('pr_closed', 'external_pr_closed') THEN 'closed'
                WHEN step = 'pr_terminal' AND status IN ('merged', 'closed', 'draft', 'open') THEN status
                WHEN step = 'external_pr_linked' AND status = 'draft' THEN 'draft'
                WHEN step = 'external_pr_linked' AND status = 'succeeded' THEN 'open'
            END,
            'publication_event_backfill',
            id,
            created_at
        FROM agent_conversation_workspace_publication_events
        WHERE CASE
                WHEN step IN ('pr_merged', 'external_pr_merged') THEN 'merged'
                WHEN step IN ('pr_closed', 'external_pr_closed') THEN 'closed'
                WHEN step = 'pr_terminal' AND status IN ('merged', 'closed', 'draft', 'open') THEN status
                WHEN step = 'external_pr_linked' AND status = 'draft' THEN 'draft'
                WHEN step = 'external_pr_linked' AND status = 'succeeded' THEN 'open'
            END IS NOT NULL;

        INSERT OR IGNORE INTO agent_conversation_workspace_state_history (
            id, conversation_id, state_family, from_state, to_state, source,
            source_event_id, created_at
        )
        SELECT
            'event:' || id || ':publication_push_status',
            conversation_id,
            'publication_push_status',
            NULL,
            CASE
                WHEN step IN ('published', 'pushed', 'external_pr_linked') AND status = 'succeeded' THEN 'pushed'
                WHEN step IN ('updated_from_base', 'base_current', 'repair_resolved') AND status = 'succeeded' THEN 'refreshed'
                WHEN step = 'no_changes' THEN 'no_changes'
                WHEN step = 'description_failed' THEN 'description_failed'
                WHEN step = 'needs_agent' THEN 'needs_agent'
                WHEN step = 'failed' THEN 'failed'
            END,
            'publication_event_backfill',
            id,
            created_at
        FROM agent_conversation_workspace_publication_events
        WHERE CASE
                WHEN step IN ('published', 'pushed', 'external_pr_linked') AND status = 'succeeded' THEN 'pushed'
                WHEN step IN ('updated_from_base', 'base_current', 'repair_resolved') AND status = 'succeeded' THEN 'refreshed'
                WHEN step = 'no_changes' THEN 'no_changes'
                WHEN step = 'description_failed' THEN 'description_failed'
                WHEN step = 'needs_agent' THEN 'needs_agent'
                WHEN step = 'failed' THEN 'failed'
            END IS NOT NULL;

        INSERT OR IGNORE INTO agent_conversation_workspace_state_history (
            id, conversation_id, state_family, from_state, to_state, source,
            source_event_id, created_at
        )
        SELECT
            'event:' || id || ':pr_supervision_status',
            conversation_id,
            'pr_supervision_status',
            NULL,
            CASE
                WHEN step = 'pr_supervision' AND status = 'enabled' THEN 'monitoring'
                WHEN step = 'pr_supervision' AND status = 'disabled' THEN 'disabled'
                WHEN step = 'pr_autofix' AND status = 'needs_agent' THEN 'fixing'
                WHEN step IN ('repair_requested', 'repair_deferred') THEN 'fixing'
                WHEN step = 'repair_sent' AND status IN ('started', 'succeeded') THEN 'fixing'
                WHEN step = 'repair_sent' AND status = 'failed' THEN 'blocked'
                WHEN step IN ('pr_autofix_completed', 'repair_completed', 'repair_resolved', 'pr_supervision_recovered') THEN 'monitoring'
            END,
            'publication_event_backfill',
            id,
            created_at
        FROM agent_conversation_workspace_publication_events
        WHERE CASE
                WHEN step = 'pr_supervision' AND status = 'enabled' THEN 'monitoring'
                WHEN step = 'pr_supervision' AND status = 'disabled' THEN 'disabled'
                WHEN step = 'pr_autofix' AND status = 'needs_agent' THEN 'fixing'
                WHEN step IN ('repair_requested', 'repair_deferred') THEN 'fixing'
                WHEN step = 'repair_sent' AND status IN ('started', 'succeeded') THEN 'fixing'
                WHEN step = 'repair_sent' AND status = 'failed' THEN 'blocked'
                WHEN step IN ('pr_autofix_completed', 'repair_completed', 'repair_resolved', 'pr_supervision_recovered') THEN 'monitoring'
            END IS NOT NULL;

        CREATE TRIGGER IF NOT EXISTS trg_agent_workspace_state_history_after_insert
        AFTER INSERT ON agent_conversation_workspaces
        BEGIN
            INSERT INTO agent_conversation_workspace_state_history (
                id, conversation_id, state_family, from_state, to_state, source,
                source_event_id, created_at
            )
            VALUES (
                lower(hex(randomblob(16))), NEW.conversation_id, 'workspace_status',
                NULL, NEW.status, 'workspace_snapshot', NULL, NEW.created_at
            );

            INSERT INTO agent_conversation_workspace_state_history (
                id, conversation_id, state_family, from_state, to_state, source,
                source_event_id, created_at
            )
            SELECT lower(hex(randomblob(16))), NEW.conversation_id, 'publication_pr_status',
                NULL, NEW.publication_pr_status, 'workspace_snapshot', NULL, NEW.updated_at
            WHERE NEW.publication_pr_status IS NOT NULL;

            INSERT INTO agent_conversation_workspace_state_history (
                id, conversation_id, state_family, from_state, to_state, source,
                source_event_id, created_at
            )
            SELECT lower(hex(randomblob(16))), NEW.conversation_id, 'publication_push_status',
                NULL, NEW.publication_push_status, 'workspace_snapshot', NULL, NEW.updated_at
            WHERE NEW.publication_push_status IS NOT NULL;

            INSERT INTO agent_conversation_workspace_state_history (
                id, conversation_id, state_family, from_state, to_state, source,
                source_event_id, created_at
            )
            SELECT lower(hex(randomblob(16))), NEW.conversation_id, 'pr_supervision_status',
                NULL, NEW.pr_supervision_status, 'workspace_snapshot', NULL,
                COALESCE(NEW.pr_supervision_updated_at, NEW.updated_at)
            WHERE NEW.pr_supervision_status IS NOT NULL;
        END;

        CREATE TRIGGER IF NOT EXISTS trg_agent_workspace_state_history_workspace_status
        AFTER UPDATE OF status ON agent_conversation_workspaces
        WHEN OLD.status IS NOT NEW.status
        BEGIN
            INSERT INTO agent_conversation_workspace_state_history (
                id, conversation_id, state_family, from_state, to_state, source,
                source_event_id, created_at
            )
            VALUES (
                lower(hex(randomblob(16))), NEW.conversation_id, 'workspace_status',
                OLD.status, COALESCE(NEW.status, 'none'), 'workspace_snapshot',
                NULL, NEW.updated_at
            );
        END;

        CREATE TRIGGER IF NOT EXISTS trg_agent_workspace_state_history_pr_status
        AFTER UPDATE OF publication_pr_status ON agent_conversation_workspaces
        WHEN OLD.publication_pr_status IS NOT NEW.publication_pr_status
        BEGIN
            INSERT INTO agent_conversation_workspace_state_history (
                id, conversation_id, state_family, from_state, to_state, source,
                source_event_id, created_at
            )
            VALUES (
                lower(hex(randomblob(16))), NEW.conversation_id, 'publication_pr_status',
                OLD.publication_pr_status, COALESCE(NEW.publication_pr_status, 'none'),
                'workspace_snapshot', NULL, NEW.updated_at
            );
        END;

        CREATE TRIGGER IF NOT EXISTS trg_agent_workspace_state_history_push_status
        AFTER UPDATE OF publication_push_status ON agent_conversation_workspaces
        WHEN OLD.publication_push_status IS NOT NEW.publication_push_status
        BEGIN
            INSERT INTO agent_conversation_workspace_state_history (
                id, conversation_id, state_family, from_state, to_state, source,
                source_event_id, created_at
            )
            VALUES (
                lower(hex(randomblob(16))), NEW.conversation_id, 'publication_push_status',
                OLD.publication_push_status, COALESCE(NEW.publication_push_status, 'none'),
                'workspace_snapshot', NULL, NEW.updated_at
            );
        END;

        CREATE TRIGGER IF NOT EXISTS trg_agent_workspace_state_history_supervision_status
        AFTER UPDATE OF pr_supervision_status ON agent_conversation_workspaces
        WHEN OLD.pr_supervision_status IS NOT NEW.pr_supervision_status
        BEGIN
            INSERT INTO agent_conversation_workspace_state_history (
                id, conversation_id, state_family, from_state, to_state, source,
                source_event_id, created_at
            )
            VALUES (
                lower(hex(randomblob(16))), NEW.conversation_id, 'pr_supervision_status',
                OLD.pr_supervision_status, COALESCE(NEW.pr_supervision_status, 'none'),
                'workspace_snapshot', NULL,
                COALESCE(NEW.pr_supervision_updated_at, NEW.updated_at)
            );
        END;

        CREATE TRIGGER IF NOT EXISTS trg_agent_workspace_state_history_publication_event
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
                    WHEN NEW.step IN ('pr_autofix_completed', 'repair_completed', 'repair_resolved', 'pr_supervision_recovered') THEN 'monitoring'
                END,
                'publication_event', NEW.id, NEW.created_at
            WHERE CASE
                    WHEN NEW.step = 'pr_supervision' AND NEW.status = 'enabled' THEN 'monitoring'
                    WHEN NEW.step = 'pr_supervision' AND NEW.status = 'disabled' THEN 'disabled'
                    WHEN NEW.step = 'pr_autofix' AND NEW.status = 'needs_agent' THEN 'fixing'
                    WHEN NEW.step IN ('repair_requested', 'repair_deferred') THEN 'fixing'
                    WHEN NEW.step = 'repair_sent' AND NEW.status IN ('started', 'succeeded') THEN 'fixing'
                    WHEN NEW.step = 'repair_sent' AND NEW.status = 'failed' THEN 'blocked'
                    WHEN NEW.step IN ('pr_autofix_completed', 'repair_completed', 'repair_resolved', 'pr_supervision_recovered') THEN 'monitoring'
                END IS NOT NULL;
        END;",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    Ok(())
}
