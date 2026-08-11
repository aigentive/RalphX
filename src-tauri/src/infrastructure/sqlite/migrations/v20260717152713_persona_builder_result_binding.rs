// Migration v20260717152713: persona builder result binding

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "chat_conversations",
        "builder_result_persona_id",
        "TEXT NULL",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_chat_conversations_builder_result_persona_id
             ON chat_conversations(builder_result_persona_id);
         UPDATE chat_conversations
         SET builder_result_persona_id = builder_draft_id,
             builder_draft_id = NULL
         WHERE builder_draft_id IS NOT NULL
           AND EXISTS (
               SELECT 1 FROM personas
               WHERE personas.id = chat_conversations.builder_draft_id
                 AND personas.status != 'draft'
           );",
    )?;
    Ok(())
}
