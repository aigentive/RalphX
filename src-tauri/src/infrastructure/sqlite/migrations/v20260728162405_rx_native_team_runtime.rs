// Migration v20260728162405: rx native team runtime

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS managed_team_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            coordinator_conversation_id TEXT NOT NULL,
            status TEXT NOT NULL,
            strategy TEXT,
            configured_concurrency INTEGER NOT NULL,
            effective_concurrency INTEGER NOT NULL,
            automatic_wake_limit INTEGER NOT NULL,
            budget_policy_json TEXT,
            pending_coordination_mode TEXT,
            pending_exit_action TEXT,
            version INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            closed_at TEXT
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_managed_team_sessions_open_conversation
            ON managed_team_sessions(coordinator_conversation_id)
            WHERE status != 'closed';

        CREATE TABLE IF NOT EXISTS managed_team_members (
            id TEXT PRIMARY KEY,
            team_id TEXT NOT NULL REFERENCES managed_team_sessions(id),
            normalized_name TEXT NOT NULL,
            name TEXT NOT NULL,
            canonical_agent_name TEXT NOT NULL,
            role_summary TEXT NOT NULL,
            harness TEXT,
            logical_model TEXT,
            logical_effort TEXT,
            delegated_session_id TEXT,
            generation INTEGER NOT NULL DEFAULT 0,
            current_run_id TEXT,
            current_assignment_id TEXT,
            status TEXT NOT NULL,
            last_activity_at TEXT,
            last_error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            stopped_at TEXT,
            UNIQUE(team_id, normalized_name)
        );
        CREATE INDEX IF NOT EXISTS idx_managed_team_members_current_run
            ON managed_team_members(team_id, generation, current_run_id);

        CREATE TABLE IF NOT EXISTS managed_team_run_bindings (
            id TEXT PRIMARY KEY,
            team_id TEXT NOT NULL REFERENCES managed_team_sessions(id),
            team_member_id TEXT REFERENCES managed_team_members(id),
            team_member_generation INTEGER,
            agent_run_id TEXT NOT NULL UNIQUE,
            conversation_id TEXT NOT NULL,
            delegated_session_id TEXT,
            trigger_kind TEXT NOT NULL,
            work_classification TEXT NOT NULL,
            assignment_id TEXT,
            first_message_sequence INTEGER,
            last_message_sequence INTEGER,
            status TEXT NOT NULL,
            version INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            created_at TEXT NOT NULL,
            launched_at TEXT,
            terminal_at TEXT,
            CHECK ((team_member_id IS NULL AND team_member_generation IS NULL AND work_classification = 'coordination_only')
                OR team_member_id IS NOT NULL)
        );
        CREATE INDEX IF NOT EXISTS idx_managed_team_run_bindings_member_generation
            ON managed_team_run_bindings(team_id, team_member_id, team_member_generation, status);

        CREATE TABLE IF NOT EXISTS managed_team_messages (
            id TEXT PRIMARY KEY,
            team_id TEXT NOT NULL REFERENCES managed_team_sessions(id),
            sequence INTEGER NOT NULL,
            sender_kind TEXT NOT NULL,
            sender_member_id TEXT,
            target_kind TEXT NOT NULL,
            target_member_id TEXT,
            kind TEXT NOT NULL,
            content TEXT NOT NULL,
            source_run_id TEXT,
            assignment_id TEXT,
            idempotency_key TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(team_id, sequence),
            UNIQUE(team_id, idempotency_key)
        );

        CREATE TABLE IF NOT EXISTS managed_team_message_deliveries (
            id TEXT PRIMARY KEY,
            message_id TEXT NOT NULL REFERENCES managed_team_messages(id),
            recipient_kind TEXT NOT NULL,
            recipient_member_id TEXT,
            recipient_generation INTEGER,
            status TEXT NOT NULL,
            queued_message_id TEXT,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            next_retry_at TEXT,
            last_error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(message_id, recipient_kind, recipient_member_id)
        );
        CREATE INDEX IF NOT EXISTS idx_managed_team_message_deliveries_actionable
            ON managed_team_message_deliveries(recipient_member_id, recipient_generation, status, next_retry_at);

        CREATE TABLE IF NOT EXISTS managed_team_wake_batches (
            id TEXT PRIMARY KEY,
            team_id TEXT NOT NULL REFERENCES managed_team_sessions(id),
            recipient_kind TEXT NOT NULL,
            recipient_member_id TEXT,
            recipient_generation INTEGER,
            first_message_sequence INTEGER NOT NULL,
            last_message_sequence INTEGER NOT NULL,
            delivery_ids_json TEXT NOT NULL,
            status TEXT NOT NULL,
            planned_agent_run_id TEXT,
            bound_agent_run_id TEXT,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            budget_count INTEGER NOT NULL DEFAULT 0,
            lease_token TEXT,
            version INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            settled_at TEXT
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_managed_team_wake_batches_active_recipient
            ON managed_team_wake_batches(team_id, recipient_kind, recipient_member_id, recipient_generation)
            WHERE status IN ('queued', 'launching', 'running');

        CREATE TABLE IF NOT EXISTS managed_team_workspace_reservations (
            id TEXT PRIMARY KEY,
            team_id TEXT NOT NULL REFERENCES managed_team_sessions(id),
            team_member_id TEXT NOT NULL REFERENCES managed_team_members(id),
            assignment_id TEXT,
            team_member_generation INTEGER NOT NULL,
            writable_paths_json TEXT NOT NULL,
            generated_outputs_json TEXT NOT NULL,
            resource_locks_json TEXT NOT NULL,
            work_classification TEXT NOT NULL,
            attempt_number INTEGER NOT NULL,
            acquired_at TEXT NOT NULL,
            released_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_managed_team_workspace_reservations_active
            ON managed_team_workspace_reservations(team_id, released_at, team_member_id, team_member_generation);
        CREATE INDEX IF NOT EXISTS idx_managed_team_workspace_reservations_assignment
            ON managed_team_workspace_reservations(assignment_id, released_at);",
    )?;

    helpers::add_column_if_not_exists(conn, "agent_task_delegate_assignments", "team_id", "TEXT")?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_task_delegate_assignments",
        "team_member_id",
        "TEXT",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_task_delegate_assignments",
        "team_member_generation",
        "INTEGER",
    )?;
    Ok(())
}
