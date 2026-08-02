//! Tests for migration v20260801211636: delegation park wake claimed at

use rusqlite::Connection;

use super::v20260801211636_delegation_park_wake_claimed_at::migrate;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE delegation_parks (
            id TEXT PRIMARY KEY,
            state TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        INSERT INTO delegation_parks (id, state, updated_at)
        VALUES ('existing', 'armed', '2026-08-01T00:00:00+00:00');",
    )
    .expect("seed delegation parks table");
    conn
}

#[test]
fn migration_adds_nullable_wake_claimed_at_without_changing_existing_rows() {
    let conn = setup_test_db();
    migrate(&conn).expect("add wake claim marker");

    let marker: Option<String> = conn
        .query_row(
            "SELECT wake_claimed_at FROM delegation_parks WHERE id = 'existing'",
            [],
            |row| row.get(0),
        )
        .expect("read existing park marker");
    assert!(marker.is_none());
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();
    migrate(&conn).expect("first migration run");
    migrate(&conn).expect("second migration run");

    let matching_columns = conn
        .prepare("PRAGMA table_info(delegation_parks)")
        .expect("prepare table info")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query columns")
        .filter_map(Result::ok)
        .filter(|column| column == "wake_claimed_at")
        .count();
    assert_eq!(matching_columns, 1);
}
