// Migration v20260712190416: branch update authority

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS git_target_leases (
            git_common_dir TEXT NOT NULL,
            target_ref TEXT NOT NULL,
            identity_version INTEGER NOT NULL DEFAULT 1 CHECK (identity_version > 0),
            owner_kind TEXT NOT NULL CHECK (owner_kind IN (
                'branch_update_operation', 'merge_attempt', 'publication_recovery', 'manual'
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

        CREATE INDEX IF NOT EXISTS idx_git_target_leases_owner
            ON git_target_leases(owner_kind, owner_id);
        CREATE INDEX IF NOT EXISTS idx_git_target_leases_active
            ON git_target_leases(released_at, recovery_state);

        CREATE TABLE IF NOT EXISTS branch_update_operations (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            direction TEXT NOT NULL CHECK (direction IN ('plan_branch', 'task_branch')),
            phase TEXT NOT NULL CHECK (phase IN (
                'programmatic', 'resolving', 'blocked', 'continuation_pending',
                'continuation_in_progress', 'settled'
            )),
            continuation TEXT NOT NULL CHECK (continuation IN (
                'resume_execution', 'resume_re_execution', 'resume_review',
                'retry_pending_merge', 'resume_waiting_on_pr',
                'finalize_post_merge_pr_publication'
            )),
            originating_history_id TEXT NOT NULL
                REFERENCES task_state_history(id) ON DELETE RESTRICT,
            attempt_id TEXT,
            source_branch TEXT NOT NULL,
            target_branch TEXT NOT NULL,
            observed_source_sha TEXT,
            observed_target_sha TEXT,
            resulting_sha TEXT,
            workspace_ownership TEXT NOT NULL CHECK (workspace_ownership IN (
                'operation_worktree', 'borrowed_task_worktree', 'borrowed_local_checkout'
            )),
            workspace_path TEXT,
            capacity_ownership TEXT NOT NULL CHECK (capacity_ownership IN (
                'inherited', 'acquired', 'released'
            )),
            failure_kind TEXT CHECK (failure_kind IS NULL OR failure_kind IN (
                'conflict', 'incomplete', 'timeout', 'branch_missing',
                'dirty_workspace', 'checkout_busy', 'workspace_ownership_invalid',
                'environment_failure', 'context_corrupt'
            )),
            conflict_files_json TEXT NOT NULL DEFAULT '[]',
            diagnostics TEXT,
            conversation_id TEXT,
            agent_run_id TEXT,
            continuation_claim_id TEXT,
            continuation_idempotency_key TEXT,
            continuation_receipt TEXT,
            git_common_dir TEXT NOT NULL,
            target_ref TEXT NOT NULL,
            target_identity_version INTEGER NOT NULL DEFAULT 1 CHECK (target_identity_version > 0),
            target_lease_epoch INTEGER NOT NULL CHECK (target_lease_epoch > 0),
            retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            settled_at TEXT
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_branch_update_operations_one_active_task
            ON branch_update_operations(task_id)
            WHERE settled_at IS NULL;
        CREATE INDEX IF NOT EXISTS idx_branch_update_operations_phase
            ON branch_update_operations(phase, updated_at);
        CREATE INDEX IF NOT EXISTS idx_branch_update_operations_target
            ON branch_update_operations(git_common_dir, target_ref, settled_at);",
    )?;
    Ok(())
}
