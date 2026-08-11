//! Tests for migration v20260806154753: add agent workspace stale base detected at

use rusqlite::Connection;

use super::v20260806154753_add_agent_workspace_stale_base_detected_at::migrate;

#[test]
fn migration_adds_an_idempotent_nullable_stale_base_detected_at() {
    let conn = Connection::open_in_memory().expect("open migration fixture");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (conversation_id TEXT PRIMARY KEY);",
    )
    .expect("seed agent conversation workspaces table");

    migrate(&conn).expect("first migration succeeds");
    migrate(&conn).expect("second migration succeeds");

    let column_names = conn
        .prepare("PRAGMA table_info(agent_conversation_workspaces)")
        .expect("prepare table inspection")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("read table columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect table columns");
    assert!(column_names
        .iter()
        .any(|column| column == "stale_base_detected_at"));
}
