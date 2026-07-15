// Migration v20260715172058: persona update draft provenance

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "personas",
        "source_persona_id",
        "TEXT REFERENCES personas(id) ON DELETE SET NULL",
    )?;
    add_column_if_not_exists(conn, "personas", "source_content_hash", "TEXT")?;
    add_column_if_not_exists(
        conn,
        "chat_conversations",
        "builder_draft_id",
        "TEXT REFERENCES personas(id) ON DELETE SET NULL",
    )?;
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_personas_slug_live;
         CREATE UNIQUE INDEX idx_personas_slug_live
             ON personas(slug) WHERE status = 'active';
         CREATE INDEX IF NOT EXISTS idx_chat_conversations_builder_draft_id
             ON chat_conversations(builder_draft_id);",
    )?;
    Ok(())
}
