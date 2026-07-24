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

    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_plan_complexity_assessments_legacy_unique;
         DROP INDEX IF EXISTS idx_plan_complexity_assessments_pair_unique;
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

    Ok(())
}
