use rusqlite::Connection;

use crate::error::{AppError, AppResult};

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "review_settings",
        "run_task_validations",
        "INTEGER NOT NULL DEFAULT 1",
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS validation_runs (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            purpose TEXT NOT NULL,
            context_type TEXT NOT NULL,
            requested_by_agent TEXT,
            status TEXT NOT NULL,
            mode TEXT NOT NULL,
            policy_enabled INTEGER NOT NULL DEFAULT 1,
            head_sha TEXT,
            base_ref TEXT,
            analysis_fingerprint TEXT,
            status_episode_entered_at TEXT,
            started_at TEXT NOT NULL,
            completed_at TEXT,
            FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS validation_command_results (
            id TEXT PRIMARY KEY,
            validation_run_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            command_source TEXT NOT NULL,
            command_ref TEXT,
            command TEXT NOT NULL,
            cwd TEXT NOT NULL,
            label TEXT,
            category TEXT NOT NULL,
            reason TEXT,
            related_files_json TEXT NOT NULL DEFAULT '[]',
            cache_key TEXT NOT NULL,
            cache_decision TEXT NOT NULL,
            status TEXT NOT NULL,
            exit_code INTEGER,
            duration_ms INTEGER,
            stdout_snippet TEXT,
            stderr_snippet TEXT,
            stdout_log_path TEXT,
            stderr_log_path TEXT,
            launcher_kind TEXT,
            resolved_shell_path TEXT,
            head_sha TEXT,
            analysis_fingerprint TEXT,
            status_episode_entered_at TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (validation_run_id) REFERENCES validation_runs(id) ON DELETE CASCADE,
            FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    helpers::create_index_if_not_exists(
        conn,
        "idx_validation_runs_task_started",
        "validation_runs",
        "task_id, started_at DESC",
    )?;
    helpers::create_index_if_not_exists(
        conn,
        "idx_validation_command_results_task_cache",
        "validation_command_results",
        "task_id, cache_key, created_at DESC",
    )?;
    helpers::create_index_if_not_exists(
        conn,
        "idx_validation_command_results_run",
        "validation_command_results",
        "validation_run_id, created_at ASC",
    )?;

    Ok(())
}
