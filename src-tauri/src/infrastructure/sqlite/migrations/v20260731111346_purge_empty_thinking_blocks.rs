// Migration v20260731111346: purge empty thinking blocks

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        r#"
        DELETE FROM chat_message_block_payloads
        WHERE block_id IN (
            SELECT id FROM chat_message_blocks
            WHERE kind = 'thinking'
              AND (
                text IS NULL
                OR TRIM(text, ' ' || char(9) || char(10) || char(11) || char(12) || char(13)) = ''
              )
        )
        "#,
        [],
    )?;
    let removed = conn.execute(
        "DELETE FROM chat_message_blocks
         WHERE kind = 'thinking'
           AND (
             text IS NULL
             OR TRIM(text, ' ' || char(9) || char(10) || char(11) || char(12) || char(13)) = ''
           )",
        [],
    )?;
    tracing::info!(removed, "purged empty thinking timeline blocks");
    Ok(())
}
