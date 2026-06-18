// Migration v70: Add base_branch_override column to plan_branches table
//
// Stores the user-selected base branch override per ideation plan.
// When set, the plan branch merges into this branch instead of the project default.
// Also fixes pre-existing gap: adds pr_eligible to the INSERT column list.

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "plan_branches", "base_branch_override", "TEXT NULL")?;

    tracing::info!("v70: added base_branch_override column to plan_branches");

    Ok(())
}
