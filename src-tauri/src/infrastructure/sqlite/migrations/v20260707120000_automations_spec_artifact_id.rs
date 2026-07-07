// Migration v20260707120000: add automations.spec_artifact_id
//
// Links an automation to a durable Specification artifact authored (or loaded)
// during setup. Plain nullable column add — no table rewrite required.

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "automations", "spec_artifact_id", "TEXT")?;
    Ok(())
}
