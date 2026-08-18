//! Tests for migration v20260810142632: agent workspace repair narrative fields

use rusqlite::Connection;

use super::v20260810142632_agent_workspace_repair_narrative_fields::migrate;

#[test]
fn migration_adds_idempotent_nullable_narrative_columns() {
    let conn = Connection::open_in_memory().expect("open migration fixture");
    conn.execute_batch("CREATE TABLE agent_workspace_repair_attempts (id TEXT PRIMARY KEY);")
        .expect("seed repair attempts table");

    migrate(&conn).expect("first migration succeeds");
    migrate(&conn).expect("second migration succeeds");

    let column_names = conn
        .prepare("PRAGMA table_info(agent_workspace_repair_attempts)")
        .expect("prepare table inspection")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("read table columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect table columns");
    assert!(column_names.iter().any(|column| column == "what_happened"));
    assert!(column_names.iter().any(|column| column == "what_i_did"));
}
