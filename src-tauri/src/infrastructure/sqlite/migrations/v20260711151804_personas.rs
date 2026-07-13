// Migration v20260711151804: personas

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS personas (
            id                TEXT PRIMARY KEY,
            slug              TEXT NOT NULL,
            name              TEXT NOT NULL,
            description       TEXT NOT NULL DEFAULT '',
            content           TEXT NOT NULL,
            status            TEXT NOT NULL DEFAULT 'draft'
                              CHECK (status IN ('draft','active','archived')),
            version           INTEGER NOT NULL DEFAULT 1,
            content_hash      TEXT NOT NULL,
            source_session_id TEXT,
            source_json       TEXT NOT NULL DEFAULT '{}',
            created_at        TEXT NOT NULL,
            updated_at        TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_personas_status ON personas(status);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_personas_slug_live
            ON personas(slug) WHERE status != 'archived';",
    )?;
    add_column_if_not_exists(conn, "chat_conversations", "persona_id", "TEXT NULL")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_chat_conversations_persona_id
             ON chat_conversations(persona_id);",
    )?;
    Ok(())
}
