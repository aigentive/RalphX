//! Tests for migration v20260730161032: agent workspace pr autofix completion evidence

use rusqlite::Connection;

use super::v20260730161032_agent_workspace_pr_autofix_completion_evidence::migrate;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn migration_adds_pr_autofix_completion_evidence_columns() {
    let conn = setup_test_db();
    conn.execute_batch("CREATE TABLE agent_workspace_repair_attempts (id TEXT PRIMARY KEY);")
        .expect("seed repair attempts table");
    migrate(&conn).expect("migration should add completion evidence columns");
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(agent_workspace_repair_attempts)")
        .expect("prepare table info")
        .query_map([], |row| row.get(1))
        .expect("query table info")
        .collect::<Result<_, _>>()
        .expect("read table info");
    assert!(columns.contains(&"pr_autofix_dispatch_head_commit".to_string()));
    assert!(columns.contains(&"pr_autofix_health_fingerprint".to_string()));
}
