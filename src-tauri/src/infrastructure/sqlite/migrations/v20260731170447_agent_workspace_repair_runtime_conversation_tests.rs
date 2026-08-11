//! Tests for migration v20260731170447: agent workspace repair runtime conversation

use rusqlite::Connection;

use super::v20260731170447_agent_workspace_repair_runtime_conversation::migrate;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_workspace_repair_attempts (\
             attempt_id TEXT PRIMARY KEY, \
             conversation_id TEXT NOT NULL, \
             generation INTEGER NOT NULL DEFAULT 0, \
             settled_at TEXT NULL\
         );",
    )
    .expect("seed agent workspace repair attempts table");
    conn
}

fn columns(conn: &Connection) -> Vec<String> {
    conn.prepare("PRAGMA table_info(agent_workspace_repair_attempts)")
        .expect("prepare table info")
        .query_map([], |row| row.get(1))
        .expect("query table info")
        .collect::<Result<_, _>>()
        .expect("read table info")
}

fn indexes(conn: &Connection) -> Vec<String> {
    conn.prepare("PRAGMA index_list(agent_workspace_repair_attempts)")
        .expect("prepare index list")
        .query_map([], |row| row.get(1))
        .expect("query index list")
        .collect::<Result<_, _>>()
        .expect("read index list")
}

#[test]
fn migration_adds_runtime_conversation_column() {
    let conn = setup_test_db();
    migrate(&conn).expect("migration should add the runtime conversation column");
    assert!(columns(&conn).contains(&"runtime_conversation_id".to_string()));
}

#[test]
fn migration_adds_runtime_conversation_lookup_index() {
    let conn = setup_test_db();
    migrate(&conn).expect("migration should add the runtime conversation index");
    assert!(indexes(&conn)
        .contains(&"idx_agent_workspace_repair_attempts_runtime_conversation".to_string()));
}

#[test]
fn migration_is_idempotent_on_already_upgraded_databases() {
    let conn = setup_test_db();
    migrate(&conn).expect("first run should add the column and index");
    migrate(&conn).expect("second run must not fail on existing column or index");
    assert_eq!(
        columns(&conn)
            .iter()
            .filter(|name| name.as_str() == "runtime_conversation_id")
            .count(),
        1
    );
}

#[test]
fn existing_attempts_stay_parent_hosted() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO agent_workspace_repair_attempts (attempt_id, conversation_id) \
         VALUES ('attempt-1', 'workspace-conversation-1')",
        [],
    )
    .expect("seed an in-flight attempt from before the upgrade");

    migrate(&conn).expect("migration should add the runtime conversation column");

    let runtime_conversation_id: Option<String> = conn
        .query_row(
            "SELECT runtime_conversation_id FROM agent_workspace_repair_attempts \
             WHERE attempt_id = 'attempt-1'",
            [],
            |row| row.get(0),
        )
        .expect("read the new column for the pre-existing attempt");
    assert!(
        runtime_conversation_id.is_none(),
        "in-flight attempts must stay NULL so they keep resolving as legacy parent-hosted runs"
    );
}
