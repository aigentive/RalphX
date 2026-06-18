use rusqlite::Connection;

use super::v20260411190000_delegated_sessions;

#[test]
fn test_delegated_sessions_migration_creates_table_and_columns() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE projects (id TEXT PRIMARY KEY);")
        .unwrap();

    v20260411190000_delegated_sessions::migrate(&conn).unwrap();

    let mut stmt = conn
        .prepare("PRAGMA table_info(delegated_sessions)")
        .unwrap();
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    for expected in [
        "id",
        "project_id",
        "parent_context_type",
        "parent_context_id",
        "agent_name",
        "harness",
        "status",
        "provider_session_id",
    ] {
        assert!(columns.iter().any(|value| value == expected));
    }
}
