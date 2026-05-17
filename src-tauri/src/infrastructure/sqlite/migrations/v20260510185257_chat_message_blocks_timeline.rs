// Migration v20260510185257: chat message blocks timeline

use rusqlite::Connection;
use serde_json::Value;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS chat_message_blocks (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
            message_id TEXT REFERENCES chat_messages(id) ON DELETE CASCADE,
            run_id TEXT,
            sequence INTEGER NOT NULL,
            block_index INTEGER NOT NULL DEFAULT 0,
            role TEXT NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('text', 'tool_use', 'task', 'system_notice', 'error')),
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

        CREATE TABLE IF NOT EXISTS chat_message_block_payloads (
            block_id TEXT PRIMARY KEY REFERENCES chat_message_blocks(id) ON DELETE CASCADE,
            input_json TEXT,
            result_json TEXT,
            raw_block_json TEXT,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_chat_message_blocks_conversation_sequence
            ON chat_message_blocks(conversation_id, sequence DESC);
        CREATE INDEX IF NOT EXISTS idx_chat_message_blocks_message
            ON chat_message_blocks(message_id, block_index);
        CREATE INDEX IF NOT EXISTS idx_chat_message_blocks_tool_call
            ON chat_message_blocks(conversation_id, tool_call_id);
        "#,
    )?;

    backfill_from_chat_messages(conn)?;
    Ok(())
}

fn backfill_from_chat_messages(conn: &Connection) -> AppResult<()> {
    if !table_exists(conn, "chat_messages")? {
        return Ok(());
    }

    let mut stmt = conn.prepare(
        r#"
        SELECT id, conversation_id, role, content, content_blocks, created_at
        FROM chat_messages
        WHERE conversation_id IS NOT NULL
        ORDER BY conversation_id ASC, created_at ASC, rowid ASC
        "#,
    )?;

    let rows = stmt
        .query_map([], |row| {
            Ok(BackfillMessage {
                id: row.get("id")?,
                conversation_id: row.get("conversation_id")?,
                role: row.get("role")?,
                content: row.get("content")?,
                content_blocks: row.get("content_blocks")?,
                created_at: row.get("created_at")?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut current_conversation = String::new();
    let mut next_sequence = 1_i64;

    for message in rows {
        if current_conversation != message.conversation_id {
            current_conversation = message.conversation_id.clone();
            next_sequence = conn.query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM chat_message_blocks WHERE conversation_id = ?1",
                [&current_conversation],
                |row| row.get(0),
            )?;
        }

        let blocks = parse_backfill_blocks(&message);
        for block in blocks {
            let block_id = format!("block:{}:{}", message.id, block.index);
            conn.execute(
                r#"
                INSERT OR IGNORE INTO chat_message_blocks (
                    id, conversation_id, message_id, run_id, sequence, block_index, role, kind, status,
                    text, tool_call_id, tool_name, tool_status, tool_input_preview,
                    tool_result_preview, metadata, provider_harness, provider_session_id,
                    created_at, updated_at, finalized_at
                )
                VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, 'finalized', ?8, ?9, ?10, ?11, ?12, ?13, NULL, NULL, NULL, ?14, ?14, ?14)
                "#,
                rusqlite::params![
                    block_id,
                    message.conversation_id,
                    message.id,
                    next_sequence,
                    block.index,
                    message.role,
                    block.kind,
                    block.text,
                    block.tool_call_id,
                    block.tool_name,
                    block.tool_status,
                    block.tool_input_preview,
                    block.tool_result_preview,
                    message.created_at,
                ],
            )?;

            if block.input_json.is_some()
                || block.result_json.is_some()
                || block.raw_block_json.is_some()
            {
                conn.execute(
                    r#"
                    INSERT OR IGNORE INTO chat_message_block_payloads (
                        block_id, input_json, result_json, raw_block_json, updated_at
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5)
                    "#,
                    rusqlite::params![
                        block_id,
                        block.input_json,
                        block.result_json,
                        block.raw_block_json,
                        message.created_at,
                    ],
                )?;
            }

            next_sequence += 1;
        }
    }

    Ok(())
}

fn table_exists(conn: &Connection, table_name: &str) -> AppResult<bool> {
    let exists: i64 = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table_name],
        |row| row.get(0),
    )?;
    Ok(exists != 0)
}

#[derive(Debug)]
struct BackfillMessage {
    id: String,
    conversation_id: String,
    role: String,
    content: String,
    content_blocks: Option<String>,
    created_at: String,
}

#[derive(Debug)]
struct BackfillBlock {
    index: i64,
    kind: &'static str,
    text: Option<String>,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    tool_status: Option<String>,
    tool_input_preview: Option<String>,
    tool_result_preview: Option<String>,
    input_json: Option<String>,
    result_json: Option<String>,
    raw_block_json: Option<String>,
}

fn parse_backfill_blocks(message: &BackfillMessage) -> Vec<BackfillBlock> {
    let parsed = message
        .content_blocks
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.as_array().cloned());

    let Some(blocks) = parsed else {
        return fallback_text_block(&message.content);
    };

    let mut normalized = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        let Some(block_type) = block.get("type").and_then(Value::as_str) else {
            continue;
        };

        match block_type {
            "text" => {
                let text = block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !text.is_empty() {
                    normalized.push(BackfillBlock {
                        index: index as i64,
                        kind: "text",
                        text: Some(text.to_string()),
                        tool_call_id: None,
                        tool_name: None,
                        tool_status: None,
                        tool_input_preview: None,
                        tool_result_preview: None,
                        input_json: None,
                        result_json: None,
                        raw_block_json: Some(block.to_string()),
                    });
                }
            }
            "tool_use" => {
                let arguments = block.get("arguments").or_else(|| block.get("input"));
                let result = block.get("result");
                normalized.push(BackfillBlock {
                    index: index as i64,
                    kind: "tool_use",
                    text: None,
                    tool_call_id: block.get("id").and_then(Value::as_str).map(str::to_string),
                    tool_name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    tool_status: Some(
                        if result.is_some() {
                            "completed"
                        } else {
                            "pending"
                        }
                        .to_string(),
                    ),
                    tool_input_preview: arguments.map(compact_preview),
                    tool_result_preview: result.map(compact_preview),
                    input_json: arguments.map(Value::to_string),
                    result_json: result.map(Value::to_string),
                    raw_block_json: Some(block.to_string()),
                });
            }
            _ => {}
        }
    }

    if normalized.is_empty() {
        fallback_text_block(&message.content)
    } else {
        normalized
    }
}

fn fallback_text_block(content: &str) -> Vec<BackfillBlock> {
    if content.is_empty() {
        return Vec::new();
    }

    vec![BackfillBlock {
        index: 0,
        kind: "text",
        text: Some(content.to_string()),
        tool_call_id: None,
        tool_name: None,
        tool_status: None,
        tool_input_preview: None,
        tool_result_preview: None,
        input_json: None,
        result_json: None,
        raw_block_json: None,
    }]
}

fn compact_preview(value: &Value) -> String {
    let raw = value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string());
    let mut preview = raw.chars().take(1_000).collect::<String>();
    if raw.chars().count() > 1_000 {
        preview.push_str("...");
    }
    preview
}
