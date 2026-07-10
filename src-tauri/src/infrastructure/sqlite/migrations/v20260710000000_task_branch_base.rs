// Migration v20260710000000: task branch base
//
// Stores the ref and exact SHA used to create a task branch so later diff,
// validation, and review gates do not resolve against a moving plan branch.

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(conn, "tasks", "task_branch_base_ref", "TEXT DEFAULT NULL")?;
    add_column_if_not_exists(conn, "tasks", "task_branch_base_sha", "TEXT DEFAULT NULL")?;
    Ok(())
}
