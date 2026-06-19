//! Tests for migration v20260616182441: external issue links

use rusqlite::Connection;

use super::v20260616182441_external_issue_links;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_creates_link_and_sync_tables() {
    let conn = setup_test_db();
    v20260616182441_external_issue_links::migrate(&conn).unwrap();

    let link_table: String = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'external_issue_links'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let sync_table: String = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'external_issue_sync_records'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(link_table, "external_issue_links");
    assert_eq!(sync_table, "external_issue_sync_records");
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260616182441_external_issue_links::migrate(&conn).unwrap();
    v20260616182441_external_issue_links::migrate(&conn).unwrap();
}

#[test]
fn test_link_idempotency_key_is_unique() {
    let conn = setup_test_db();
    v20260616182441_external_issue_links::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO external_issue_links (
            id, provider, external_kind, external_id, local_object_kind,
            local_object_id, idempotency_key
        ) VALUES ('link-1', 'linear', 'issue', 'lin_1', 'task', 'task-1', 'idem-1')",
        [],
    )
    .unwrap();

    let duplicate = conn.execute(
        "INSERT INTO external_issue_links (
            id, provider, external_kind, external_id, local_object_kind,
            local_object_id, idempotency_key
        ) VALUES ('link-2', 'linear', 'issue', 'lin_2', 'task', 'task-2', 'idem-1')",
        [],
    );

    assert!(duplicate.is_err());
}
