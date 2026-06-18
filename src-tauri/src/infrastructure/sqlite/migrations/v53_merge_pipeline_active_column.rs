// Migration v53: Add merge_pipeline_active column to tasks table
//
// Previously stored as a JSON key inside task.metadata, which was vulnerable
// to a race condition where concurrent metadata writers would clobber the flag.
// Dedicated column eliminates the read-modify-write race entirely.

use rusqlite::Connection;

use super::helpers;
use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "tasks", "merge_pipeline_active", "TEXT DEFAULT NULL")?;

    tracing::info!("v53: added merge_pipeline_active column to tasks table");

    Ok(())
}
