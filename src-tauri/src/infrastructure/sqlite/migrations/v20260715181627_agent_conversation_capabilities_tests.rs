//! Tests for migration v20260715181627: agent conversation capabilities

use rusqlite::Connection;

use super::v20260715181627_agent_conversation_capabilities;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn migration_creates_both_capabilities_explicitly_disabled_for_fresh_databases() {
    let conn = setup_test_db();
    v20260715181627_agent_conversation_capabilities::migrate(&conn)
        .expect("migration should succeed");

    let values: (i64, i64) = conn
        .query_row(
            "SELECT agent_conversation_team, agent_conversation_workflows
             FROM ui_feature_flag_overrides WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("singleton capability row should exist");

    assert_eq!(values, (0, 0));
}

#[test]
fn migration_forces_both_capabilities_off_for_existing_installations() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE ui_feature_flag_overrides (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            agent_personas INTEGER NULL
         );
         INSERT INTO ui_feature_flag_overrides (id, agent_personas) VALUES (1, 1);",
    )
    .expect("legacy override table should be created");

    v20260715181627_agent_conversation_capabilities::migrate(&conn)
        .expect("upgrade migration should succeed");

    let values: (Option<i64>, i64, i64) = conn
        .query_row(
            "SELECT agent_personas, agent_conversation_team,
                    agent_conversation_workflows
             FROM ui_feature_flag_overrides WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("upgraded singleton row should exist");

    assert_eq!(values, (Some(1), 0, 0));
}

#[test]
fn migration_is_idempotent_without_reenabling_capabilities() {
    let conn = setup_test_db();
    v20260715181627_agent_conversation_capabilities::migrate(&conn)
        .expect("first migration should succeed");
    conn.execute(
        "UPDATE ui_feature_flag_overrides
         SET agent_conversation_team = 1, agent_conversation_workflows = 1
         WHERE id = 1",
        [],
    )
    .expect("explicit settings update should succeed");

    v20260715181627_agent_conversation_capabilities::migrate(&conn).expect("rerun should succeed");

    let values: (i64, i64) = conn
        .query_row(
            "SELECT agent_conversation_team, agent_conversation_workflows
             FROM ui_feature_flag_overrides WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("singleton capability row should exist");

    assert_eq!(values, (1, 1));
}

#[test]
fn migration_widens_conversation_capabilities_without_losing_rows_or_indexes() {
    let conn = setup_test_db();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE chat_conversations (
            id TEXT PRIMARY KEY,
            coordination_mode TEXT NOT NULL DEFAULT 'solo'
                CHECK(coordination_mode IN ('solo', 'legacy_claude_team', 'rx_native_team'))
         );
         CREATE INDEX idx_chat_conversations_coordination_mode
             ON chat_conversations(coordination_mode);
         CREATE TABLE chat_messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
         );
         INSERT INTO chat_conversations (id, coordination_mode)
             VALUES ('conversation-1', 'rx_native_team');
         INSERT INTO chat_messages (id, conversation_id)
             VALUES ('message-1', 'conversation-1');",
    )
    .expect("legacy conversation schema should be created");

    v20260715181627_agent_conversation_capabilities::migrate(&conn)
        .expect("capability migration should widen the constraint");

    for (id, mode) in [
        ("conversation-workflow", "rx_native_workflow"),
        ("conversation-ultra", "codex_native_ultra"),
    ] {
        conn.execute(
            "INSERT INTO chat_conversations (id, coordination_mode) VALUES (?1, ?2)",
            [id, mode],
        )
        .expect("new capability mode should satisfy the migrated constraint");
    }

    let existing_mode: String = conn
        .query_row(
            "SELECT coordination_mode FROM chat_conversations WHERE id = 'conversation-1'",
            [],
            |row| row.get(0),
        )
        .expect("existing conversation should survive migration");
    let message_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chat_messages WHERE conversation_id = 'conversation-1'",
            [],
            |row| row.get(0),
        )
        .expect("dependent messages should remain readable");
    let index_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_chat_conversations_coordination_mode'",
            [],
            |row| row.get(0),
        )
        .expect("conversation index should be queryable");
    let foreign_keys_enabled: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .expect("foreign key mode should be queryable");

    assert_eq!(existing_mode, "rx_native_team");
    assert_eq!(message_count, 1);
    assert_eq!(index_count, 1);
    assert_eq!(foreign_keys_enabled, 1);
}
