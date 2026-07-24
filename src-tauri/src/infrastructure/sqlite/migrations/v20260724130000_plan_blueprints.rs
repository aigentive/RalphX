use rusqlite::Connection;

use crate::error::{AppError, AppResult};

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    let adds_contract_version =
        !helpers::column_exists(conn, "ideation_sessions", "plan_contract_version");

    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "plan_blueprint_artifact_id",
        "TEXT REFERENCES artifacts(id) ON DELETE SET NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "inherited_plan_blueprint_artifact_id",
        "TEXT REFERENCES artifacts(id) ON DELETE SET NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "verified_plan_blueprint_artifact_id",
        "TEXT REFERENCES artifacts(id) ON DELETE SET NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "blueprint_version_last_read",
        "INTEGER",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "plan_contract_version",
        "INTEGER NOT NULL DEFAULT 2 CHECK(plan_contract_version IN (1, 2))",
    )?;
    if adds_contract_version {
        conn.execute("UPDATE ideation_sessions SET plan_contract_version = 1", [])
            .map_err(|error| AppError::Database(error.to_string()))?;
    }

    helpers::add_column_if_not_exists(
        conn,
        "plan_artifact_approvals",
        "blueprint_artifact_id",
        "TEXT REFERENCES artifacts(id) ON DELETE CASCADE",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "plan_artifact_approvals",
        "blueprint_artifact_version",
        "INTEGER",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "task_proposals",
        "blueprint_artifact_id",
        "TEXT REFERENCES artifacts(id) ON DELETE SET NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "task_proposals",
        "blueprint_version_at_creation",
        "INTEGER",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "tasks",
        "plan_blueprint_artifact_id",
        "TEXT REFERENCES artifacts(id) ON DELETE SET NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "deferred_plan_approval_notifications",
        "plan_target_id",
        "TEXT",
    )?;
    conn.execute(
        "UPDATE deferred_plan_approval_notifications
         SET plan_target_id = artifact_id
         WHERE plan_target_id IS NULL",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    helpers::add_column_if_not_exists(
        conn,
        "automation_runs",
        "plan_last_parked_blueprint_artifact_id",
        "TEXT REFERENCES artifacts(id) ON DELETE SET NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "plan_complexity_assessments",
        "blueprint_artifact_id",
        "TEXT REFERENCES artifacts(id) ON DELETE CASCADE",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "plan_complexity_assessments",
        "blueprint_artifact_version",
        "INTEGER",
    )?;

    rebuild_plan_complexity_assessments(conn)?;

    Ok(())
}

fn rebuild_plan_complexity_assessments(conn: &Connection) -> AppResult<()> {
    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(error.to_string()))?;
    transaction
        .execute_batch(
            "DROP TABLE IF EXISTS plan_complexity_assessments_new;
             CREATE TABLE plan_complexity_assessments_new (
                 id TEXT PRIMARY KEY,
                 session_id TEXT NOT NULL REFERENCES ideation_sessions(id) ON DELETE CASCADE,
                 artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
                 artifact_version INTEGER NOT NULL,
                 blueprint_artifact_id TEXT REFERENCES artifacts(id) ON DELETE CASCADE,
                 blueprint_artifact_version INTEGER,
                 level TEXT NOT NULL CHECK(level IN (
                     'trivial', 'simple', 'moderate', 'complex', 'very_complex'
                 )),
                 score INTEGER NOT NULL CHECK(score >= 0 AND score <= 100),
                 recommended_action TEXT NOT NULL CHECK(recommended_action IN (
                     'implement_directly', 'create_proposals'
                 )),
                 confidence REAL NOT NULL CHECK(confidence >= 0.0 AND confidence <= 1.0),
                 reason_summary TEXT NOT NULL,
                 signals_json TEXT NOT NULL DEFAULT '{}',
                 assessed_by TEXT NOT NULL DEFAULT 'ralphx-utility-plan-complexity',
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             INSERT INTO plan_complexity_assessments_new (
                 id,
                 session_id,
                 artifact_id,
                 artifact_version,
                 blueprint_artifact_id,
                 blueprint_artifact_version,
                 level,
                 score,
                 recommended_action,
                 confidence,
                 reason_summary,
                 signals_json,
                 assessed_by,
                 created_at,
                 updated_at
             )
             SELECT
                 id,
                 session_id,
                 artifact_id,
                 artifact_version,
                 blueprint_artifact_id,
                 blueprint_artifact_version,
                 level,
                 score,
                 recommended_action,
                 confidence,
                 reason_summary,
                 signals_json,
                 assessed_by,
                 created_at,
                 updated_at
             FROM plan_complexity_assessments;
             DROP TABLE plan_complexity_assessments;
             ALTER TABLE plan_complexity_assessments_new
                 RENAME TO plan_complexity_assessments;
             CREATE INDEX idx_plan_complexity_assessments_session
                 ON plan_complexity_assessments(session_id, updated_at DESC);
             CREATE INDEX idx_plan_complexity_assessments_artifact
                 ON plan_complexity_assessments(artifact_id, artifact_version);
             CREATE UNIQUE INDEX idx_plan_complexity_assessments_legacy_unique
                 ON plan_complexity_assessments(session_id, artifact_id, artifact_version)
                 WHERE blueprint_artifact_id IS NULL;
             CREATE UNIQUE INDEX idx_plan_complexity_assessments_pair_unique
                 ON plan_complexity_assessments(
                     session_id,
                     artifact_id,
                     artifact_version,
                     blueprint_artifact_id,
                     blueprint_artifact_version
                 )
                 WHERE blueprint_artifact_id IS NOT NULL;",
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    transaction
        .commit()
        .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(())
}
