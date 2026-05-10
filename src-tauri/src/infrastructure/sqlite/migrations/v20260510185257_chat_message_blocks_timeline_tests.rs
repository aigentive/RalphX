//! Tests for migration v20260510185257: chat message blocks timeline

use rusqlite::Connection;

use super::v20260510185257_chat_message_blocks_timeline;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    v20260510185257_chat_message_blocks_timeline::migrate(&conn).unwrap();

    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('chat_message_blocks', 'chat_message_block_payloads')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 2);
}

#[test]
fn test_migration_backfills_content_blocks_as_individual_timeline_items() {
    let conn = setup_test_db();
    create_minimal_chat_tables(&conn);

    conn.execute(
        "INSERT INTO chat_conversations (id) VALUES ('conversation-1')",
        [],
    )
    .unwrap();
    conn.execute(
        r#"
        INSERT INTO chat_messages (id, conversation_id, role, content, content_blocks, created_at)
        VALUES (
            'message-1',
            'conversation-1',
            'orchestrator',
            'intro',
            ?1,
            '2026-05-10T10:00:00+00:00'
        )
        "#,
        [r#"
        [
          {"type":"text","text":"checking"},
          {"type":"tool_use","id":"tool-1","name":"exec_command","arguments":{"cmd":"date"},"result":"ok"},
          {"type":"text","text":"done"}
        ]
        "#],
    )
    .unwrap();

    v20260510185257_chat_message_blocks_timeline::migrate(&conn).unwrap();

    let blocks: Vec<(i64, String, Option<String>, Option<String>)> = {
        let mut stmt = conn
            .prepare(
                "SELECT sequence, kind, text, tool_name FROM chat_message_blocks ORDER BY sequence ASC",
            )
            .unwrap();
        stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };

    assert_eq!(
        blocks,
        vec![
            (1, "text".to_string(), Some("checking".to_string()), None),
            (
                2,
                "tool_use".to_string(),
                None,
                Some("exec_command".to_string())
            ),
            (3, "text".to_string(), Some("done".to_string()), None),
        ]
    );

    let payload_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chat_message_block_payloads WHERE block_id = 'block:message-1:1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(payload_count, 1);
}

#[test]
fn test_migration_backfill_is_idempotent() {
    let conn = setup_test_db();
    create_minimal_chat_tables(&conn);

    conn.execute(
        "INSERT INTO chat_conversations (id) VALUES ('conversation-1')",
        [],
    )
    .unwrap();
    conn.execute(
        r#"
        INSERT INTO chat_messages (id, conversation_id, role, content, created_at)
        VALUES ('message-1', 'conversation-1', 'user', 'hello', '2026-05-10T10:00:00+00:00')
        "#,
        [],
    )
    .unwrap();

    v20260510185257_chat_message_blocks_timeline::migrate(&conn).unwrap();
    v20260510185257_chat_message_blocks_timeline::migrate(&conn).unwrap();

    let block_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chat_message_blocks", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(block_count, 1);
}

fn create_minimal_chat_tables(conn: &Connection) {
    conn.execute("CREATE TABLE chat_conversations (id TEXT PRIMARY KEY)", [])
        .unwrap();
    conn.execute(
        r#"
        CREATE TABLE chat_messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            content_blocks TEXT,
            created_at TEXT NOT NULL
        )
        "#,
        [],
    )
    .unwrap();
}
