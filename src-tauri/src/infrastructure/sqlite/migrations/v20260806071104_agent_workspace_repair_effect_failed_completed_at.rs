// Migration v20260806071104: agent workspace repair effect failed completed at
//
// A `Failed` repair effect previously could never carry `completed_at`, so it permanently
// occupied the attempt's single open-effect slot (`idx_agent_workspace_repair_effects_one_open`
// keys on `completed_at IS NULL`). This rebuild lets a failed effect close (`completed_at` set,
// `receipt_json` still forbidden) so a terminated push effect releases its attempt's slot for a
// fresh retry.

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "BEGIN IMMEDIATE;

        DROP TABLE IF EXISTS agent_workspace_repair_effects_rebuild;
        CREATE TABLE agent_workspace_repair_effects_rebuild (
            id TEXT PRIMARY KEY,
            attempt_id TEXT NOT NULL
                REFERENCES agent_workspace_repair_attempts(id) ON DELETE CASCADE,
            kind TEXT NOT NULL CHECK (kind IN (
                'push_branch', 'create_pr', 'update_pr', 'restore_auto_merge'
            )),
            status TEXT NOT NULL CHECK (status IN ('pending', 'in_flight', 'observed', 'failed')),
            idempotency_key TEXT NOT NULL UNIQUE,
            intended_head_oid TEXT,
            expected_remote_oid TEXT,
            expected_pr_number INTEGER,
            expected_remote_absent INTEGER NOT NULL DEFAULT 0
                CHECK (expected_remote_absent IN (0, 1)),
            receipt_json TEXT,
            last_error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            completed_at TEXT,
            CHECK (NOT expected_remote_absent OR expected_remote_oid IS NULL),
            CHECK (
                (status = 'observed' AND completed_at IS NOT NULL AND receipt_json IS NOT NULL)
                OR (status = 'failed' AND receipt_json IS NULL)
                OR (status IN ('pending', 'in_flight') AND completed_at IS NULL)
            )
        );
        INSERT INTO agent_workspace_repair_effects_rebuild (
            id, attempt_id, kind, status, idempotency_key, intended_head_oid,
            expected_remote_oid, expected_pr_number, expected_remote_absent, receipt_json,
            last_error, created_at, updated_at, completed_at
        )
        SELECT
            id, attempt_id, kind, status, idempotency_key, intended_head_oid,
            expected_remote_oid, expected_pr_number, expected_remote_absent, receipt_json,
            last_error, created_at, updated_at,
            CASE
                WHEN status = 'failed' AND completed_at IS NULL THEN updated_at
                ELSE completed_at
            END
        FROM agent_workspace_repair_effects;
        DROP TABLE agent_workspace_repair_effects;
        ALTER TABLE agent_workspace_repair_effects_rebuild RENAME TO agent_workspace_repair_effects;
        CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_workspace_repair_effects_one_open
            ON agent_workspace_repair_effects(attempt_id)
            WHERE completed_at IS NULL;

        COMMIT;",
    )?;
    Ok(())
}
