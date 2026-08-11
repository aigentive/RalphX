// Helper functions for task repository operations

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::domain::entities::{InternalStatus, TaskId};
use crate::domain::repositories::StateHistoryMetadata;
use crate::error::{AppError, AppResult};

/// Persist a status change: UPDATE task + INSERT history record.
///
/// Caller MUST wrap this in `db.run_transaction()` for atomicity.
pub(super) fn persist_status_change(
    conn: &Connection,
    id: &TaskId,
    from: InternalStatus,
    to: InternalStatus,
    trigger: &str,
    now: DateTime<Utc>,
) -> AppResult<String> {
    // Update task status
    conn.execute(
        "UPDATE tasks SET internal_status = ?2, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![id.as_str(), to.as_str(), now.to_rfc3339()],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    insert_status_history(conn, id, from, to, trigger, now)
}

pub(super) fn insert_status_history(
    conn: &Connection,
    id: &TaskId,
    from: InternalStatus,
    to: InternalStatus,
    trigger: &str,
    now: DateTime<Utc>,
) -> AppResult<String> {
    let history_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, from_status, to_status, changed_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            history_id,
            id.as_str(),
            from.as_str(),
            to.as_str(),
            trigger,
            now.to_rfc3339()
        ],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(history_id)
}

/// Update the metadata of the most recent state history entry for a task
///
/// This finds the latest state history entry by task_id and created_at,
/// then updates its metadata JSON column with conversation_id and agent_run_id.
pub(super) fn update_latest_state_history_metadata_sync(
    conn: &Connection,
    task_id: &TaskId,
    metadata: &StateHistoryMetadata,
) -> AppResult<()> {
    // Build JSON for metadata
    let metadata_json = serde_json::json!({
        "conversation_id": metadata.conversation_id,
        "agent_run_id": metadata.agent_run_id
    })
    .to_string();

    // Update the most recent state history entry for this task
    // We use a subquery to find the id of the latest entry
    let rows_affected = conn
        .execute(
            "UPDATE task_state_history
         SET metadata = ?2
         WHERE id = (
             SELECT id FROM task_state_history
             WHERE task_id = ?1
             ORDER BY created_at DESC
             LIMIT 1
         )",
            rusqlite::params![task_id.as_str(), metadata_json],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

    if rows_affected == 0 {
        return Err(AppError::Database(format!(
            "No state history entry found for task {}",
            task_id.as_str()
        )));
    }

    Ok(())
}
