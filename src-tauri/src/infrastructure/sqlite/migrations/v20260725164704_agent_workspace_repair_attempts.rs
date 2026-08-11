// Migration v20260725164704: agent workspace repair attempts

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "BEGIN IMMEDIATE;

        DROP TABLE IF EXISTS git_target_leases_repair_owner_rebuild;
        CREATE TABLE git_target_leases_repair_owner_rebuild (
            git_common_dir TEXT NOT NULL,
            target_ref TEXT NOT NULL,
            identity_version INTEGER NOT NULL DEFAULT 1 CHECK (identity_version > 0),
            owner_kind TEXT NOT NULL CHECK (owner_kind IN (
                'branch_update_operation', 'merge_attempt', 'publication_recovery',
                'agent_workspace_repair', 'manual'
            )),
            owner_task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
            owner_id TEXT NOT NULL,
            fencing_epoch INTEGER NOT NULL CHECK (fencing_epoch > 0),
            acquired_at TEXT NOT NULL,
            recovery_state TEXT NOT NULL CHECK (recovery_state IN (
                'ready', 'mutation_in_flight', 'reconciling'
            )),
            mutation_claim_id TEXT,
            mutation_kind TEXT CHECK (mutation_kind IS NULL OR mutation_kind IN (
                'fetch', 'merge', 'rebase', 'push', 'worktree_create',
                'worktree_delete', 'abort', 'cleanup'
            )),
            mutation_process_group_id INTEGER,
            mutation_started_at TEXT,
            released_at TEXT,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            PRIMARY KEY (git_common_dir, target_ref),
            CHECK (
                (mutation_claim_id IS NULL AND mutation_kind IS NULL AND mutation_started_at IS NULL)
                OR
                (mutation_claim_id IS NOT NULL AND mutation_kind IS NOT NULL AND mutation_started_at IS NOT NULL)
            )
        );
        INSERT INTO git_target_leases_repair_owner_rebuild (
            git_common_dir, target_ref, identity_version, owner_kind, owner_task_id,
            owner_id, fencing_epoch, acquired_at, recovery_state, mutation_claim_id,
            mutation_kind, mutation_process_group_id, mutation_started_at, released_at, updated_at
        )
        SELECT
            git_common_dir, target_ref, identity_version, owner_kind, owner_task_id,
            owner_id, fencing_epoch, acquired_at, recovery_state, mutation_claim_id,
            mutation_kind, mutation_process_group_id, mutation_started_at, released_at, updated_at
        FROM git_target_leases;
        DROP TABLE git_target_leases;
        ALTER TABLE git_target_leases_repair_owner_rebuild RENAME TO git_target_leases;
        CREATE INDEX IF NOT EXISTS idx_git_target_leases_owner
            ON git_target_leases(owner_kind, owner_id);
        CREATE INDEX IF NOT EXISTS idx_git_target_leases_active
            ON git_target_leases(released_at, recovery_state);

        CREATE TABLE IF NOT EXISTS agent_workspace_repair_attempts (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL
                REFERENCES agent_conversation_workspaces(conversation_id) ON DELETE CASCADE,
            generation INTEGER NOT NULL CHECK (generation > 0),
            source TEXT NOT NULL CHECK (source IN (
                'base_update', 'publish', 'pr_conflict', 'pr_autofix', 'legacy'
            )),
            phase TEXT NOT NULL CHECK (phase IN (
                'requested', 'dispatching', 'repairing', 'validating', 'awaiting_review',
                'continuation_pending', 'continuing', 'ready', 'blocked'
            )),
            continuation TEXT NOT NULL CHECK (continuation IN (
                'update_only', 'publish', 'resume_pr_supervision', 'manual'
            )),
            reserved_agent_run_id TEXT,
            target_base_ref TEXT NOT NULL,
            target_base_commit TEXT,
            pending_reasons_json TEXT NOT NULL DEFAULT '[]'
                CHECK (json_valid(pending_reasons_json) AND json_type(pending_reasons_json) = 'array'),
            review_required INTEGER NOT NULL CHECK (review_required IN (0, 1)),
            auto_publish_enabled INTEGER NOT NULL CHECK (auto_publish_enabled IN (0, 1)),
            auto_merge_desired INTEGER NOT NULL CHECK (auto_merge_desired IN (0, 1)),
            auto_merge_method TEXT,
            dispatch_count INTEGER NOT NULL DEFAULT 0 CHECK (dispatch_count >= 0),
            next_dispatch_at TEXT,
            repair_head_commit TEXT,
            summary TEXT,
            blocker TEXT,
            git_common_dir TEXT,
            target_ref TEXT,
            target_identity_version INTEGER,
            target_lease_epoch INTEGER,
            outcome TEXT CHECK (outcome IS NULL OR outcome IN (
                'succeeded', 'superseded', 'failed', 'cancelled'
            )),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            settled_at TEXT,
            UNIQUE (conversation_id, generation),
            CHECK (
                (outcome IS NULL AND settled_at IS NULL)
                OR (outcome IS NOT NULL AND settled_at IS NOT NULL)
            )
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_workspace_repair_attempts_one_active
            ON agent_workspace_repair_attempts(conversation_id)
            WHERE settled_at IS NULL;
        CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_workspace_repair_attempts_reserved_run
            ON agent_workspace_repair_attempts(reserved_agent_run_id)
            WHERE reserved_agent_run_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_agent_workspace_repair_attempts_phase_updated
            ON agent_workspace_repair_attempts(phase, updated_at);
        CREATE INDEX IF NOT EXISTS idx_agent_workspace_repair_attempts_target
            ON agent_workspace_repair_attempts(git_common_dir, target_ref, settled_at);

        CREATE TABLE IF NOT EXISTS agent_workspace_repair_effects (
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
                OR (status IN ('pending', 'in_flight', 'failed') AND completed_at IS NULL)
            )
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_workspace_repair_effects_one_open
            ON agent_workspace_repair_effects(attempt_id)
            WHERE completed_at IS NULL;

        COMMIT;",
    )?;
    Ok(())
}
