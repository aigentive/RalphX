// Migration v20260730025727: chat message blocks thinking kind

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = OFF;
        BEGIN IMMEDIATE;

        CREATE TABLE chat_message_blocks_new (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
            message_id TEXT REFERENCES chat_messages(id) ON DELETE CASCADE,
            run_id TEXT,
            sequence INTEGER NOT NULL,
            block_index INTEGER NOT NULL DEFAULT 0,
            role TEXT NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('text', 'tool_use', 'task', 'system_notice', 'error', 'thinking')),
            status TEXT NOT NULL CHECK (status IN ('streaming', 'finalized', 'error')),
            text TEXT,
            tool_call_id TEXT,
            tool_name TEXT,
            tool_status TEXT,
            tool_input_preview TEXT,
            tool_result_preview TEXT,
            metadata TEXT,
            provider_harness TEXT,
            provider_session_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            finalized_at TEXT,
            UNIQUE(conversation_id, sequence),
            UNIQUE(message_id, block_index)
        );

        INSERT INTO chat_message_blocks_new (
            id, conversation_id, message_id, run_id, sequence, block_index, role, kind, status,
            text, tool_call_id, tool_name, tool_status, tool_input_preview, tool_result_preview,
            metadata, provider_harness, provider_session_id, created_at, updated_at, finalized_at
        )
        SELECT id, conversation_id, message_id, run_id, sequence, block_index, role, kind, status,
               text, tool_call_id, tool_name, tool_status, tool_input_preview, tool_result_preview,
               metadata, provider_harness, provider_session_id, created_at, updated_at, finalized_at
        FROM chat_message_blocks;

        DROP TABLE chat_message_blocks;
        ALTER TABLE chat_message_blocks_new RENAME TO chat_message_blocks;

        CREATE INDEX idx_chat_message_blocks_conversation_sequence
            ON chat_message_blocks(conversation_id, sequence DESC);
        CREATE INDEX idx_chat_message_blocks_message
            ON chat_message_blocks(message_id, block_index);
        CREATE INDEX idx_chat_message_blocks_tool_call
            ON chat_message_blocks(conversation_id, tool_call_id);

        COMMIT;
        PRAGMA foreign_keys = ON;
        "#,
    )?;
    Ok(())
}
