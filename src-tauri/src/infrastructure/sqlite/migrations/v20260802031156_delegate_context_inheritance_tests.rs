//! Tests for migration v20260802031156: delegate context inheritance

use rusqlite::Connection;

use super::v20260802031156_delegate_context_inheritance::migrate;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE delegated_sessions (id TEXT PRIMARY KEY);
         INSERT INTO delegated_sessions (id) VALUES ('upgraded-session');",
    )
    .expect("seed legacy delegated sessions table");
    conn
}

#[test]
fn migration_preserves_upgraded_sessions_with_context_defaults() {
    let conn = setup_test_db();
    migrate(&conn).expect("migrate delegated session context");

    let (authorized, caller_conversation_id): (i64, Option<String>) = conn
        .query_row(
            "SELECT delegate_context_authorized, caller_conversation_id
             FROM delegated_sessions
             WHERE id = 'upgraded-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read upgraded delegated session");
    assert_eq!(authorized, 1);
    assert!(caller_conversation_id.is_none());

    conn.execute(
        "INSERT INTO delegated_sessions (id) VALUES ('new-session')",
        [],
    )
    .expect("insert migrated delegated session");
    let authorized: i64 = conn
        .query_row(
            "SELECT delegate_context_authorized FROM delegated_sessions WHERE id = 'new-session'",
            [],
            |row| row.get(0),
        )
        .expect("read delegated session default");
    assert_eq!(authorized, 1);
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();
    migrate(&conn).expect("first migration run");
    migrate(&conn).expect("second migration run");

    let columns = conn
        .prepare("PRAGMA table_info(delegated_sessions)")
        .expect("prepare table info")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query table info")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect table columns");
    assert_eq!(
        columns
            .iter()
            .filter(|column| column.as_str() == "delegate_context_authorized")
            .count(),
        1
    );
    assert_eq!(
        columns
            .iter()
            .filter(|column| column.as_str() == "caller_conversation_id")
            .count(),
        1
    );
}
