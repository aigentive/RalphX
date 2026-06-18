// Migration v20260328194000: Add first-class provenance fields for follow-up ideation sessions
//
// Adds source_task_id, source_context_type, source_context_id, and spawn_reason to
// ideation_sessions so execution/review/merge-originated follow-up sessions can carry
// explicit provenance without mutating accepted parent sessions or relying on metadata blobs.

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "ideation_sessions", "source_task_id", "TEXT")?;
    helpers::add_column_if_not_exists(conn, "ideation_sessions", "source_context_type", "TEXT")?;
    helpers::add_column_if_not_exists(conn, "ideation_sessions", "source_context_id", "TEXT")?;
    helpers::add_column_if_not_exists(conn, "ideation_sessions", "spawn_reason", "TEXT")?;

    tracing::info!(
        "v20260328194000: added ideation follow-up provenance columns to ideation_sessions"
    );

    Ok(())
}
