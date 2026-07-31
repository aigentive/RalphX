//! Tests for migration v20260731125157: add workspace repair fingerprint state

use rusqlite::Connection;

use super::v20260731125157_add_workspace_repair_fingerprint_state::migrate;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (conversation_id TEXT PRIMARY KEY);",
    )
    .expect("seed agent conversation workspaces table");
    conn
}

fn columns(conn: &Connection) -> Vec<String> {
    conn.prepare("PRAGMA table_info(agent_conversation_workspaces)")
        .expect("prepare table info")
        .query_map([], |row| row.get(1))
        .expect("query table info")
        .collect::<Result<_, _>>()
        .expect("read table info")
}

#[test]
fn migration_adds_last_blocked_pr_health_columns() {
    let conn = setup_test_db();
    migrate(&conn).expect("migration should add fingerprint state columns");
    let columns = columns(&conn);
    assert!(columns.contains(&"last_blocked_pr_health_fingerprint".to_string()));
    assert!(columns.contains(&"last_blocked_pr_health_at".to_string()));
}

#[test]
fn migration_is_idempotent_on_already_upgraded_databases() {
    let conn = setup_test_db();
    migrate(&conn).expect("first run should add columns");
    migrate(&conn).expect("second run must not fail on existing columns");
    assert_eq!(
        columns(&conn)
            .iter()
            .filter(|name| name.as_str() == "last_blocked_pr_health_fingerprint")
            .count(),
        1
    );
}

#[test]
fn migration_preserves_existing_workspace_rows() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO agent_conversation_workspaces (conversation_id) VALUES ('existing')",
        [],
    )
    .expect("seed an already-published workspace");
    migrate(&conn).expect("migration should add fingerprint state columns");
    let fingerprint: Option<String> = conn
        .query_row(
            "SELECT last_blocked_pr_health_fingerprint FROM agent_conversation_workspaces \
             WHERE conversation_id = 'existing'",
            [],
            |row| row.get(0),
        )
        .expect("read backfilled column");
    assert!(
        fingerprint.is_none(),
        "existing workspaces start with no remembered failure identity"
    );
}
