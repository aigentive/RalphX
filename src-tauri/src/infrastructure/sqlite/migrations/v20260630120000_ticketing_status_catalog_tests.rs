//! Tests for migration v20260630120000: ticketing status catalog

use rusqlite::Connection;

use super::v20260630120000_ticketing_status_catalog;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_creates_ticketing_status_catalog() {
    let conn = setup_test_db();
    v20260630120000_ticketing_status_catalog::migrate(&conn).unwrap();

    let table: String = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'ticketing_status_catalog'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(table, "ticketing_status_catalog");

    let columns: Vec<String> = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(ticketing_status_catalog)")
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };

    for column in [
        "provider",
        "scope_kind",
        "scope_id",
        "provider_status_id",
        "provider_status_name",
        "provider_category",
        "provider_color",
        "provider_order",
        "display_order",
        "color_override",
        "is_visible",
        "is_terminal",
        "last_seen_at",
        "stale_since",
        "metadata_json",
    ] {
        assert!(
            columns.iter().any(|existing| existing == column),
            "missing ticketing_status_catalog.{column}"
        );
    }
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260630120000_ticketing_status_catalog::migrate(&conn).unwrap();
    v20260630120000_ticketing_status_catalog::migrate(&conn).unwrap();
}

#[test]
fn test_provider_scope_status_identity_is_unique() {
    let conn = setup_test_db();
    v20260630120000_ticketing_status_catalog::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ticketing_status_catalog (
            id, provider, scope_kind, scope_id, provider_status_id,
            provider_status_name, provider_category, display_order
        ) VALUES (
            'row-1', 'jira', 'jira_project', 'RX', '10001', 'To Do', 'todo', 0
        )",
        [],
    )
    .unwrap();

    let duplicate = conn.execute(
        "INSERT INTO ticketing_status_catalog (
            id, provider, scope_kind, scope_id, provider_status_id,
            provider_status_name, provider_category, display_order
        ) VALUES (
            'row-2', 'jira', 'jira_project', 'RX', '10001', 'Ready', 'todo', 1
        )",
        [],
    );

    assert!(duplicate.is_err());
}
