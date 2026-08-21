//! Tests for migration v20260821101236: agent runs conversation started index

use rusqlite::Connection;

use super::helpers::index_exists;
use super::v20260821101236_agent_runs_conversation_started_index;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_runs (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            started_at TEXT NOT NULL
        );",
    )
    .expect("Failed to create agent_runs table");
    conn
}

#[test]
fn test_migration_creates_conversation_started_index() {
    let conn = setup_test_db();
    assert!(!index_exists(&conn, "idx_agent_runs_conversation_started"));

    v20260821101236_agent_runs_conversation_started_index::migrate(&conn).unwrap();

    assert!(index_exists(&conn, "idx_agent_runs_conversation_started"));
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();

    v20260821101236_agent_runs_conversation_started_index::migrate(&conn).unwrap();
    v20260821101236_agent_runs_conversation_started_index::migrate(&conn).unwrap();

    assert!(index_exists(&conn, "idx_agent_runs_conversation_started"));
}
