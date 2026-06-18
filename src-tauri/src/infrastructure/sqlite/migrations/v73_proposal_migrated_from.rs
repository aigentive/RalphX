// Migration v73: Add migrated_from traceability columns to task_proposals table
//
// Supports the proposal migration/export feature. When a proposal is copied
// from one session to another via migrate_proposals, these fields record the
// origin for traceability and idempotency checks.

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "task_proposals", "migrated_from_session_id", "TEXT")?;
    helpers::add_column_if_not_exists(conn, "task_proposals", "migrated_from_proposal_id", "TEXT")?;

    tracing::info!(
        "v73: added migrated_from_session_id and migrated_from_proposal_id columns to task_proposals"
    );

    Ok(())
}
