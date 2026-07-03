//! Tests for migration v20260620075610: provider ticket operations

use rusqlite::Connection;

use super::v20260620075610_provider_ticket_operations;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

fn create_external_issue_links_stub(conn: &Connection) {
    conn.execute(
        "CREATE TABLE external_issue_links (id TEXT PRIMARY KEY)",
        [],
    )
    .unwrap();
}

#[test]
fn test_migration_creates_provider_ticket_operation_history() {
    let conn = setup_test_db();
    v20260620075610_provider_ticket_operations::migrate(&conn).unwrap();

    let table: String = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'provider_ticket_operations'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(table, "provider_ticket_operations");

    let columns: Vec<String> = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(provider_ticket_operations)")
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };

    for column in [
        "provider",
        "external_kind",
        "external_id",
        "external_key",
        "link_id",
        "local_project_id",
        "operation",
        "client_operation_id",
        "status",
        "provider_operation_id",
        "error_message",
        "metadata_json",
    ] {
        assert!(
            columns.iter().any(|existing| existing == column),
            "missing provider_ticket_operations.{column}"
        );
    }
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260620075610_provider_ticket_operations::migrate(&conn).unwrap();
    v20260620075610_provider_ticket_operations::migrate(&conn).unwrap();
}

#[test]
fn test_client_operation_id_is_unique() {
    let conn = setup_test_db();
    create_external_issue_links_stub(&conn);
    v20260620075610_provider_ticket_operations::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO provider_ticket_operations (
            id, provider, external_kind, external_id, operation, client_operation_id, status
        ) VALUES (
            'operation-1', 'linear', 'issue', 'lin_1', 'comment', 'client-op-1', 'pending'
        )",
        [],
    )
    .unwrap();

    let duplicate = conn.execute(
        "INSERT INTO provider_ticket_operations (
            id, provider, external_kind, external_id, operation, client_operation_id, status
        ) VALUES (
            'operation-2', 'linear', 'issue', 'lin_2', 'comment', 'client-op-1', 'pending'
        )",
        [],
    );

    assert!(duplicate.is_err());
}
