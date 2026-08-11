// Migration v20260722132100: automation run goal item

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "automation_runs", "goal_item_id", "TEXT")?;
    Ok(())
}
