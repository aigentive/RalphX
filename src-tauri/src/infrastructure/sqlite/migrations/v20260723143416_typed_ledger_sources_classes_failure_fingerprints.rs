// Migration v20260723143416: typed ledger sources/classes and failure fingerprints.

use rusqlite::Connection;

use super::v20260521222911_agent_plan_mode::{foreign_keys_enabled, legacy_alter_table_enabled};
use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    let foreign_keys_was_enabled = foreign_keys_enabled(conn)?;
    let legacy_alter_table_was_enabled = legacy_alter_table_enabled(conn)?;

    conn.execute_batch("PRAGMA foreign_keys = OFF; PRAGMA legacy_alter_table = ON;")
        .map_err(db_error)?;
    if let Err(error) = conn.execute_batch("BEGIN IMMEDIATE") {
        return restore_pragmas(
            conn,
            foreign_keys_was_enabled,
            legacy_alter_table_was_enabled,
            Err(AppError::Database(format!(
                "BEGIN IMMEDIATE failed: {error}"
            ))),
        );
    }

    let transaction_result = match migrate_in_transaction(conn) {
        Ok(()) => conn
            .execute_batch("COMMIT")
            .map_err(|error| AppError::Database(format!("COMMIT failed: {error}"))),
        Err(primary) => {
            let rollback = conn.execute_batch("ROLLBACK");
            Err(match rollback {
                Ok(()) => primary,
                Err(error) => {
                    AppError::Database(format!("{primary}; additionally ROLLBACK failed: {error}"))
                }
            })
        }
    };

    restore_pragmas(
        conn,
        foreign_keys_was_enabled,
        legacy_alter_table_was_enabled,
        transaction_result,
    )
}

fn migrate_in_transaction(conn: &Connection) -> AppResult<()> {
    rebuild_task_outcomes(conn)?;
    rebuild_skill_usage_events(conn)?;
    ensure_foreign_keys_valid(conn)
}

fn rebuild_task_outcomes(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE task_outcomes_d3 (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            source TEXT NOT NULL CHECK (source IN (
                'agent_session',
                'agent_workspace',
                'agent_workspace_pr',
                'github_pr_review',
                'agent_conversation',
                'review',
                'git_commit_history',
                'github_pr_history',
                'plan_mode',
                'merge',
                'merge_validation',
                'verification',
                'task_pipeline'
            )),
            source_ref_kind TEXT NOT NULL,
            source_ref_id TEXT NOT NULL,
            task_id TEXT,
            conversation_id TEXT,
            agent_run_id TEXT,
            pull_request_id TEXT,
            proposal_id TEXT,
            verification_id TEXT,
            review_id TEXT,
            outcome_class TEXT,
            status TEXT NOT NULL CHECK (status IN (
                'unknown', 'eligible', 'ineligible', 'succeeded', 'failed'
            )),
            evidence_json TEXT NOT NULL DEFAULT '{}',
            failure_fingerprint TEXT CHECK (
                failure_fingerprint IS NULL
                OR (
                    length(failure_fingerprint) = 64
                    AND failure_fingerprint = lower(failure_fingerprint)
                    AND failure_fingerprint NOT GLOB '*[^0-9a-f]*'
                )
            ),
            provider_harness TEXT,
            provider_session_id TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            CHECK (length(source_ref_kind) > 0),
            CHECK (length(source_ref_id) > 0)
        );
        INSERT INTO task_outcomes_d3 (
            id, project_id, source, source_ref_kind, source_ref_id, task_id,
            conversation_id, agent_run_id, pull_request_id, proposal_id,
            verification_id, review_id, outcome_class, status, evidence_json,
            failure_fingerprint, provider_harness, provider_session_id,
            created_at, updated_at
        )
        SELECT
            id,
            project_id,
            CASE source
                WHEN 'qa' THEN 'verification'
                WHEN 'workspace_review' THEN 'review'
                WHEN 'ideation' THEN 'plan_mode'
                ELSE source
            END,
            source_ref_kind,
            source_ref_id,
            task_id,
            conversation_id,
            agent_run_id,
            pull_request_id,
            proposal_id,
            verification_id,
            review_id,
            outcome_class,
            status,
            evidence_json,
            NULL,
            provider_harness,
            provider_session_id,
            created_at,
            updated_at
        FROM task_outcomes;
        DROP TABLE task_outcomes;
        ALTER TABLE task_outcomes_d3 RENAME TO task_outcomes;

        CREATE UNIQUE INDEX idx_task_outcomes_source_ref
            ON task_outcomes(project_id, source, source_ref_kind, source_ref_id);
        CREATE INDEX idx_task_outcomes_project_status
            ON task_outcomes(project_id, status, updated_at DESC);
        CREATE INDEX idx_task_outcomes_failure_fingerprint
            ON task_outcomes(project_id, failure_fingerprint, updated_at DESC)
            WHERE failure_fingerprint IS NOT NULL;",
    )
    .map_err(db_error)
}

fn rebuild_skill_usage_events(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE skill_usage_events_d3 (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            project_skill_id TEXT NOT NULL REFERENCES project_skills(id) ON DELETE CASCADE,
            conversation_id TEXT,
            agent_run_id TEXT,
            provider_harness TEXT,
            stage TEXT,
            bucket TEXT,
            injection_kind TEXT NOT NULL CHECK (injection_kind IN (
                'compact_index',
                'full_load',
                'composer_directive',
                'interactive_stdin_unattributed'
            )),
            outcome_id TEXT REFERENCES task_outcomes(id) ON DELETE SET NULL,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        INSERT INTO skill_usage_events_d3 (
            id, project_id, project_skill_id, conversation_id, agent_run_id,
            provider_harness, stage, bucket, injection_kind, outcome_id,
            metadata_json, created_at
        )
        SELECT
            id,
            project_id,
            project_skill_id,
            conversation_id,
            agent_run_id,
            provider_harness,
            stage,
            bucket,
            CASE injection_kind
                WHEN 'explicit' THEN 'full_load'
                ELSE injection_kind
            END,
            outcome_id,
            metadata_json,
            created_at
        FROM skill_usage_events;
        DROP TABLE skill_usage_events;
        ALTER TABLE skill_usage_events_d3 RENAME TO skill_usage_events;

        CREATE INDEX idx_skill_usage_events_project_created
            ON skill_usage_events(project_id, created_at DESC);
        CREATE INDEX idx_skill_usage_events_skill_created
            ON skill_usage_events(project_skill_id, created_at DESC);
        CREATE INDEX idx_skill_usage_events_run
            ON skill_usage_events(agent_run_id)
            WHERE agent_run_id IS NOT NULL;",
    )
    .map_err(db_error)
}

fn ensure_foreign_keys_valid(conn: &Connection) -> AppResult<()> {
    let mut statement = conn.prepare("PRAGMA foreign_key_check").map_err(db_error)?;
    let mut rows = statement.query([]).map_err(db_error)?;
    if let Some(row) = rows.next().map_err(db_error)? {
        let table: String = row.get(0).map_err(db_error)?;
        return Err(AppError::Database(format!(
            "foreign key check failed for table {table}"
        )));
    }
    Ok(())
}

fn restore_pragmas(
    conn: &Connection,
    foreign_keys_was_enabled: bool,
    legacy_alter_table_was_enabled: bool,
    result: AppResult<()>,
) -> AppResult<()> {
    let restore = conn.execute_batch(&format!(
        "PRAGMA legacy_alter_table = {}; PRAGMA foreign_keys = {};",
        if legacy_alter_table_was_enabled {
            "ON"
        } else {
            "OFF"
        },
        if foreign_keys_was_enabled {
            "ON"
        } else {
            "OFF"
        },
    ));
    match (result, restore) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(()), Err(error)) => Err(AppError::Database(format!(
            "failed to restore migration PRAGMAs: {error}"
        ))),
        (Err(primary), Err(error)) => Err(AppError::Database(format!(
            "{primary}; additionally failed to restore migration PRAGMAs: {error}"
        ))),
    }
}

fn db_error(error: rusqlite::Error) -> AppError {
    AppError::Database(error.to_string())
}
