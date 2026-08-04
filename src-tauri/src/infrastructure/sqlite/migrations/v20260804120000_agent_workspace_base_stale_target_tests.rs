use rusqlite::Connection;

use super::v20260804120000_agent_workspace_base_stale_target::migrate;

#[test]
fn migration_adds_an_idempotent_nullable_base_stale_target() {
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
    assert!(column_names
        .iter()
        .any(|column| column == "base_update_target_commit"));
}
