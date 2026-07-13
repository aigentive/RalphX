//! Tests for migration v20260712093952: delegated session working directory

use rusqlite::Connection;

use super::v20260712093952_delegated_session_working_directory;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    conn.execute_batch("CREATE TABLE delegated_sessions (id TEXT PRIMARY KEY);")
        .unwrap();
    v20260712093952_delegated_session_working_directory::migrate(&conn).unwrap();
    let columns = conn
        .prepare("PRAGMA table_info(delegated_sessions)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(columns.iter().any(|column| column == "working_directory"));
}
