// Migration v20260804073002: jira link acceptance criteria backfill

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "UPDATE agent_conversation_jira_issue_links
            SET refresh_status = 'not_loaded'
          WHERE refresh_status = 'loaded'
            AND (
                acceptance_criteria_markdown IS NULL
                OR TRIM(acceptance_criteria_markdown) = ''
            )",
        [],
    )?;
    Ok(())
}
