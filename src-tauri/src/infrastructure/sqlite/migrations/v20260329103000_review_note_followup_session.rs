// Migration v20260329103000: Persist spawned follow-up ideation session IDs on review notes.

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "review_notes", "followup_session_id", "TEXT")?;

    tracing::info!("v20260329103000: added followup_session_id column to review_notes");

    Ok(())
}
