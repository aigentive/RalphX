use rusqlite::Connection;

use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::migrations::v20260521222911_agent_plan_mode::{
    foreign_keys_enabled, legacy_alter_table_enabled, rewrite_table_check_constraint,
};

use super::helpers;

pub(super) const SINGLE_OPEN_ALWAYS_OPEN_RUN_STATUSES: &[&str] = &[
    "pending",
    "provisioning",
    "running",
    "awaiting_plan_approval",
    "published",
];
pub(super) const SINGLE_OPEN_SIGNAL_TERMINAL_RUN_STATUSES: &[&str] =
    &["merged", "pr_closed", "agent_failed"];
pub(super) const SINGLE_OPEN_UNRESOLVED_JUDGE_STATES: &[&str] = &["none", "in_progress", "failed"];

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_automation_columns(conn)?;
    add_run_columns(conn)?;
    widen_run_status_check(conn)?;
    rebuild_single_open_index(conn)?;
    Ok(())
}

#[cfg(test)]
pub(super) fn single_open_index_includes(status: &str, judge_state: &str) -> bool {
    SINGLE_OPEN_ALWAYS_OPEN_RUN_STATUSES.contains(&status)
        || (SINGLE_OPEN_SIGNAL_TERMINAL_RUN_STATUSES.contains(&status)
            && SINGLE_OPEN_UNRESOLVED_JUDGE_STATES.contains(&judge_state))
}

fn add_automation_columns(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "automations",
        "plan_approval_mode",
        "TEXT NOT NULL DEFAULT 'manual' CHECK (plan_approval_mode IN ('manual','automatic'))",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "automations",
        "pr_merge_mode",
        "TEXT NOT NULL DEFAULT 'manual' CHECK (pr_merge_mode IN ('manual','automatic'))",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "automations",
        "plan_deep_verification",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}

fn add_run_columns(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "automation_runs",
        "plan_judge_state",
        "TEXT NOT NULL DEFAULT 'none' CHECK (plan_judge_state IN ('none','in_progress','done','failed'))",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "automation_runs",
        "plan_judge_lease_expires_at",
        "TEXT",
    )?;
    helpers::add_column_if_not_exists(conn, "automation_runs", "plan_judge_verdict_json", "TEXT")?;
    helpers::add_column_if_not_exists(
        conn,
        "automation_runs",
        "plan_revision_round",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "automation_runs",
        "plan_reminder_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "automation_runs",
        "plan_pending_instructions",
        "TEXT",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "automation_runs",
        "plan_last_parked_artifact_id",
        "TEXT",
    )?;
    helpers::add_column_if_not_exists(conn, "automation_runs", "agent_phase_started_at", "TEXT")?;
    Ok(())
}

fn widen_run_status_check(conn: &Connection) -> AppResult<()> {
    let foreign_keys_was_enabled = foreign_keys_enabled(conn)?;
    let legacy_alter_table_was_enabled = legacy_alter_table_enabled(conn)?;
    conn.execute("PRAGMA foreign_keys = OFF", [])
        .map_err(|error| AppError::Database(error.to_string()))?;
    conn.execute("PRAGMA legacy_alter_table = ON", [])
        .map_err(|error| AppError::Database(error.to_string()))?;

    let migrate_result = rewrite_table_check_constraint(
        conn,
        "automation_runs",
        "'awaiting_plan_approval'",
        &[
            (
                "CHECK (status IN ('pending','provisioning','running','published','completed','merged','pr_closed','agent_failed','cancelled'))",
                "CHECK (status IN ('pending','provisioning','running','awaiting_plan_approval','published','completed','merged','pr_closed','agent_failed','cancelled'))",
            ),
            (
                "CHECK (status IN ('pending','provisioning','running','published','merged','pr_closed','agent_failed','cancelled'))",
                "CHECK (status IN ('pending','provisioning','running','awaiting_plan_approval','published','merged','pr_closed','agent_failed','cancelled'))",
            ),
        ],
        "automation run status",
    );

    let restore_legacy_result = conn
        .execute(
            if legacy_alter_table_was_enabled {
                "PRAGMA legacy_alter_table = ON"
            } else {
                "PRAGMA legacy_alter_table = OFF"
            },
            [],
        )
        .map(|_| ())
        .map_err(|error| AppError::Database(error.to_string()));
    let restore_foreign_keys_result = conn
        .execute(
            if foreign_keys_was_enabled {
                "PRAGMA foreign_keys = ON"
            } else {
                "PRAGMA foreign_keys = OFF"
            },
            [],
        )
        .map(|_| ())
        .map_err(|error| AppError::Database(error.to_string()));

    migrate_result?;
    restore_legacy_result?;
    restore_foreign_keys_result?;
    Ok(())
}

fn rebuild_single_open_index(conn: &Connection) -> AppResult<()> {
    conn.execute("DROP INDEX IF EXISTS idx_automation_runs_single_open", [])
        .map_err(|error| AppError::Database(error.to_string()))?;
    let index_sql = automation_run_single_open_index_sql();
    conn.execute(&index_sql, [])
        .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(())
}

fn automation_run_single_open_index_sql() -> String {
    format!(
        "CREATE UNIQUE INDEX idx_automation_runs_single_open
         ON automation_runs(automation_id)
         WHERE status IN ({always_open})
            OR (status IN ({signal_terminal})
                AND judge_state IN ({unresolved_judge}))",
        always_open = sql_in_list(SINGLE_OPEN_ALWAYS_OPEN_RUN_STATUSES),
        signal_terminal = sql_in_list(SINGLE_OPEN_SIGNAL_TERMINAL_RUN_STATUSES),
        unresolved_judge = sql_in_list(SINGLE_OPEN_UNRESOLVED_JUDGE_STATES),
    )
}

fn sql_in_list(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| format!("'{value}'"))
        .collect::<Vec<_>>()
        .join(",")
}
