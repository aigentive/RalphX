// Migration v20260330000000: add verification_confirmation_status to ideation_sessions
use super::helpers;
use crate::error::AppResult;
use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "verification_confirmation_status",
        "TEXT",
    )?;
    tracing::info!("v20260330000000: added verification_confirmation_status to ideation_sessions");
    Ok(())
}
