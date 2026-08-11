// Migration v20260715170000: automation authoring state

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "automations", "authoring_state_json", "TEXT")?;
    Ok(())
}
