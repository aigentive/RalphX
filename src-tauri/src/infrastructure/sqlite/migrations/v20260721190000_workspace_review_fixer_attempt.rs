use rusqlite::Connection;

use crate::{
    error::AppResult, infrastructure::sqlite::migrations::helpers::add_column_if_not_exists,
};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "agent_workspace_review_monitors",
        "review_fixer_attempt_id",
        "TEXT NULL",
    )?;
    conn.execute(
        "UPDATE agent_workspace_review_monitors
         SET review_fixer_status = 'failed',
             last_error = COALESCE(
                 last_error,
                 'Workspace Review fixer routing predates durable attempt attribution'
             )
         WHERE review_fixer_status = 'routing'
           AND review_fixer_attempt_id IS NULL",
        [],
    )?;
    Ok(())
}
