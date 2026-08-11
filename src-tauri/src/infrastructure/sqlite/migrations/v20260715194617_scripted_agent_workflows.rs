// Migration v20260715194617: scripted agent workflows

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_workflow_scripts (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
            project_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            script_source TEXT NOT NULL CHECK(length(trim(script_source)) > 0),
            script_hash TEXT NOT NULL CHECK(length(script_hash) = 64),
            protocol_version INTEGER NOT NULL CHECK(protocol_version = 1),
            meta_json TEXT NOT NULL,
            permission_summary_json TEXT NOT NULL,
            permission_hash TEXT NOT NULL CHECK(length(permission_hash) = 64),
            estimated_fanout INTEGER NOT NULL DEFAULT 0 CHECK(estimated_fanout BETWEEN 0 AND 1000),
            approved_script_hash TEXT NULL,
            approved_permission_hash TEXT NULL,
            approved_at TEXT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
         );

         CREATE INDEX IF NOT EXISTS idx_agent_workflow_scripts_conversation
             ON agent_workflow_scripts(conversation_id, updated_at DESC);

         CREATE TRIGGER IF NOT EXISTS trg_agent_workflow_script_edit_invalidates_approval
         AFTER UPDATE OF script_source, script_hash, permission_summary_json, permission_hash
         ON agent_workflow_scripts
         WHEN OLD.script_hash <> NEW.script_hash OR OLD.permission_hash <> NEW.permission_hash
         BEGIN
            UPDATE agent_workflow_scripts
            SET approved_script_hash = NULL,
                approved_permission_hash = NULL,
                approved_at = NULL
            WHERE id = NEW.id;
         END;

         CREATE TABLE IF NOT EXISTS agent_workflow_runs (
            id TEXT PRIMARY KEY,
            script_id TEXT NOT NULL REFERENCES agent_workflow_scripts(id) ON DELETE CASCADE,
            conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
            project_id TEXT NOT NULL,
            harness TEXT NOT NULL CHECK(harness IN ('claude', 'codex')),
            script_hash TEXT NOT NULL CHECK(length(script_hash) = 64),
            permission_hash TEXT NOT NULL CHECK(length(permission_hash) = 64),
            args_json TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN (
                'awaiting_approval', 'queued', 'running', 'pause_requested',
                'paused', 'recovering', 'completed', 'failed', 'cancelled', 'disabled'
            )),
            attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt >= 0),
            runner_instance_id TEXT NULL,
            lease_expires_at TEXT NULL,
            heartbeat_at TEXT NULL,
            pause_requested INTEGER NOT NULL DEFAULT 0 CHECK(pause_requested IN (0, 1)),
            cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK(cancel_requested IN (0, 1)),
            result_json TEXT NULL,
            error TEXT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            completed_at TEXT NULL
         );

         CREATE INDEX IF NOT EXISTS idx_agent_workflow_runs_recovery
             ON agent_workflow_runs(status, lease_expires_at);
         CREATE INDEX IF NOT EXISTS idx_agent_workflow_runs_conversation
             ON agent_workflow_runs(conversation_id, created_at DESC);

         CREATE TABLE IF NOT EXISTS agent_workflow_phases (
            id TEXT PRIMARY KEY,
            run_id TEXT NOT NULL REFERENCES agent_workflow_runs(id) ON DELETE CASCADE,
            phase_key TEXT NOT NULL,
            name TEXT NOT NULL,
            ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
            status TEXT NOT NULL CHECK(status IN (
                'pending', 'running', 'completed', 'failed', 'cancelled', 'skipped'
            )),
            started_at TEXT NULL,
            completed_at TEXT NULL,
            error TEXT NULL,
            UNIQUE(run_id, phase_key)
         );

         CREATE INDEX IF NOT EXISTS idx_agent_workflow_phases_run
             ON agent_workflow_phases(run_id, ordinal);

         CREATE TABLE IF NOT EXISTS agent_workflow_invocations (
            id TEXT PRIMARY KEY,
            run_id TEXT NOT NULL REFERENCES agent_workflow_runs(id) ON DELETE CASCADE,
            phase_id TEXT NULL REFERENCES agent_workflow_phases(id) ON DELETE SET NULL,
            logical_key TEXT NOT NULL,
            agent_name TEXT NOT NULL,
            prompt_hash TEXT NOT NULL CHECK(length(prompt_hash) = 64),
            schema_hash TEXT NULL,
            status TEXT NOT NULL CHECK(status IN (
                'pending', 'running', 'completed', 'failed', 'cancelled', 'skipped'
            )),
            delegated_session_id TEXT NULL REFERENCES delegated_sessions(id) ON DELETE SET NULL,
            child_conversation_id TEXT NULL REFERENCES chat_conversations(id) ON DELETE SET NULL,
            result_json TEXT NULL,
            error TEXT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            completed_at TEXT NULL,
            UNIQUE(run_id, logical_key)
         );

         CREATE INDEX IF NOT EXISTS idx_agent_workflow_invocations_run
             ON agent_workflow_invocations(run_id, created_at);

         CREATE TABLE IF NOT EXISTS agent_workflow_logs (
            run_id TEXT NOT NULL REFERENCES agent_workflow_runs(id) ON DELETE CASCADE,
            sequence INTEGER NOT NULL CHECK(sequence >= 0),
            level TEXT NOT NULL CHECK(level IN ('debug', 'info', 'warn', 'error')),
            message TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY(run_id, sequence)
         );

         CREATE INDEX IF NOT EXISTS idx_agent_workflow_logs_run
             ON agent_workflow_logs(run_id, sequence);",
    )?;
    Ok(())
}
