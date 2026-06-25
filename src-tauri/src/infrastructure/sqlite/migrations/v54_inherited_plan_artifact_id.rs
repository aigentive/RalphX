// Migration v54: Add inherited_plan_artifact_id column to ideation_sessions
//
// Separates owned vs inherited plan artifacts for child sessions.
// When a child session inherits a parent's plan via `inherit_context: true`:
//   - plan_artifact_id = None (child has no own plan yet)
//   - inherited_plan_artifact_id = parent's plan artifact ID (read-only)
//
// Data migration: For existing child sessions where plan_artifact_id matches
// the parent's artifact, move to inherited_plan_artifact_id and set
// plan_artifact_id = NULL.

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "ideation_sessions",
        "inherited_plan_artifact_id",
        "TEXT DEFAULT NULL",
    )?;

    // Data migration: move inherited plan_artifact_id to the new column
    // for existing child sessions where the artifact matches the parent's artifact.
    conn.execute(
        "UPDATE ideation_sessions
         SET inherited_plan_artifact_id = plan_artifact_id,
             plan_artifact_id = NULL
         WHERE parent_session_id IS NOT NULL
           AND plan_artifact_id IS NOT NULL
           AND plan_artifact_id IN (
               SELECT parent.plan_artifact_id
               FROM ideation_sessions AS parent
               WHERE parent.id = ideation_sessions.parent_session_id
                 AND parent.plan_artifact_id IS NOT NULL
           )",
        [],
    )
    .map_err(|e| crate::error::AppError::Database(e.to_string()))?;

    tracing::info!("v54: added inherited_plan_artifact_id column to ideation_sessions and migrated existing child session data");

    Ok(())
}
