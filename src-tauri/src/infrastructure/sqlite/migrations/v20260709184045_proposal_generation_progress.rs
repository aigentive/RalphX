// Migration v20260709184045: proposal generation progress

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "proposal_generation_status",
        "TEXT NOT NULL DEFAULT 'idle'",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "proposal_generation_phase",
        "TEXT",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "proposal_generation_expected_count",
        "INTEGER",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "proposal_generation_created_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "proposal_generation_dependency_count",
        "INTEGER",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "proposal_generation_error",
        "TEXT",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "proposal_generation_started_at",
        "TEXT",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "proposal_generation_updated_at",
        "TEXT",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "proposal_generation_completed_at",
        "TEXT",
    )?;

    tracing::info!("v20260709184045: added proposal generation progress columns");

    Ok(())
}
