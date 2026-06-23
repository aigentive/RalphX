//! Tests for migration v20260621201947: ticket canonical branches

use rusqlite::Connection;

use super::helpers::{column_exists, table_exists};
use super::v20260621201947_ticket_canonical_branches;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn creates_table() {
    let conn = setup_test_db();

    v20260621201947_ticket_canonical_branches::migrate(&conn).unwrap();

    assert!(table_exists(&conn, "ticket_canonical_branches"));
}

#[test]
fn creates_all_columns() {
    let conn = setup_test_db();

    v20260621201947_ticket_canonical_branches::migrate(&conn).unwrap();

    for column in [
        "project_id",
        "provider",
        "issue_key",
        "branch_name",
        "base_branch",
        "base_commit",
        "origin_pushed",
        "terminal",
        "created_at",
        "updated_at",
    ] {
        assert!(
            column_exists(&conn, "ticket_canonical_branches", column),
            "missing column: {column}"
        );
    }
}

#[test]
fn primary_key_is_project_provider_issue_key() {
    let conn = setup_test_db();

    v20260621201947_ticket_canonical_branches::migrate(&conn).unwrap();

    let pk_columns: Vec<String> = {
        let mut statement = conn
            .prepare("SELECT name FROM pragma_table_info('ticket_canonical_branches') WHERE pk > 0 ORDER BY pk")
            .unwrap();
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows
    };

    assert_eq!(pk_columns, vec!["project_id", "provider", "issue_key"]);
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();

    v20260621201947_ticket_canonical_branches::migrate(&conn).unwrap();
    v20260621201947_ticket_canonical_branches::migrate(&conn).unwrap();

    assert!(table_exists(&conn, "ticket_canonical_branches"));
}
