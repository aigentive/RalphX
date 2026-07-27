//! Tests for migration v20260722022339: usage capture provenance and raw snapshots

use rusqlite::Connection;

use super::v20260722022339_usage_capture_provenance_and_raw_snapshots;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_runs (
            id TEXT PRIMARY KEY,
            input_tokens INTEGER,
            output_tokens INTEGER,
            cache_creation_tokens INTEGER,
            cache_read_tokens INTEGER,
            estimated_usd REAL
        );
        CREATE TABLE chat_messages (
            id TEXT PRIMARY KEY,
            input_tokens INTEGER,
            output_tokens INTEGER,
            cache_creation_tokens INTEGER,
            cache_read_tokens INTEGER,
            estimated_usd REAL
        );",
    )
    .unwrap();
    conn
}

#[test]
fn migration_adds_capture_columns_without_mutating_legacy_usage() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO agent_runs (id, input_tokens, cache_read_tokens) VALUES ('run-1', 100, 80)",
        [],
    )
    .unwrap();

    v20260722022339_usage_capture_provenance_and_raw_snapshots::migrate(&conn).unwrap();

    let legacy: (Option<i64>, Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT input_tokens, cache_read_tokens, usage_provenance FROM agent_runs WHERE id = 'run-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(legacy, (Some(100), Some(80), None));

    for table in ["agent_runs", "chat_messages"] {
        let columns: Vec<String> = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for expected in [
            "usage_provenance",
            "raw_usage_input_tokens",
            "raw_usage_output_tokens",
            "raw_usage_cache_creation_tokens",
            "raw_usage_cache_read_tokens",
            "raw_usage_estimated_usd",
        ] {
            assert!(columns.iter().any(|column| column == expected));
        }
    }
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();

    v20260722022339_usage_capture_provenance_and_raw_snapshots::migrate(&conn).unwrap();
    v20260722022339_usage_capture_provenance_and_raw_snapshots::migrate(&conn).unwrap();
}
