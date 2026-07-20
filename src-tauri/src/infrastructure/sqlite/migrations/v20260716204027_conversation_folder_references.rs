// Migration v20260716204027: conversation folder references

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE conversation_folder_references (
            id TEXT PRIMARY KEY NOT NULL,
            conversation_id TEXT NOT NULL,
            folder_path TEXT NOT NULL,
            display_name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            removed_at TEXT NULL,
            FOREIGN KEY (conversation_id) REFERENCES chat_conversations(id) ON DELETE CASCADE
        );
        CREATE INDEX idx_conversation_folder_references_conversation_id
            ON conversation_folder_references(conversation_id);
        CREATE UNIQUE INDEX idx_conversation_folder_references_live_path
            ON conversation_folder_references(conversation_id, folder_path)
            WHERE removed_at IS NULL;
        ALTER TABLE ui_feature_flag_overrides
            ADD COLUMN composer_folder_references INTEGER NULL;",
    )?;
    Ok(())
}
