// Migration v20260715013854: model native plan verification

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "ideation_settings",
        "auto_verify_plans",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_not_exists(
        conn,
        "ideation_settings",
        "ext_auto_verify_plans",
        "INTEGER NULL DEFAULT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "verified_plan_artifact_id",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "verified_plan_agent_run_id",
        "TEXT NULL",
    )?;
    add_column_if_not_exists(conn, "agent_runs", "action_kind", "TEXT NULL")?;
    add_column_if_not_exists(conn, "agent_runs", "action_context_id", "TEXT NULL")?;
    add_column_if_not_exists(conn, "agent_runs", "action_target_id", "TEXT NULL")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_agent_runs_action_lookup
         ON agent_runs(action_kind, action_context_id, action_target_id, started_at DESC);",
    )?;

    conn.execute(
        "UPDATE ideation_sessions
         SET verified_plan_artifact_id = plan_artifact_id
         WHERE plan_artifact_id IS NOT NULL
           AND verification_status IN ('verified', 'imported_verified')",
        [],
    )?;
    conn.execute(
        "UPDATE ideation_sessions
         SET status = 'archived',
             archived_at = COALESCE(archived_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         WHERE session_purpose = 'verification'",
        [],
    )?;
    conn.execute(
        "UPDATE ideation_sessions
         SET verification_in_progress = 0,
             verification_status = CASE
                 WHEN verification_status IN ('verified', 'imported_verified')
                     THEN verification_status
                 ELSE 'unverified'
             END",
        [],
    )?;
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS clear_failed_plan_verification_proof
         AFTER UPDATE OF status ON agent_runs
         WHEN NEW.status IN ('failed', 'cancelled')
         BEGIN
           UPDATE ideation_sessions
           SET verified_plan_artifact_id = NULL,
               verified_plan_agent_run_id = NULL
           WHERE verified_plan_agent_run_id = NEW.id;
         END;",
    )?;
    Ok(())
}
