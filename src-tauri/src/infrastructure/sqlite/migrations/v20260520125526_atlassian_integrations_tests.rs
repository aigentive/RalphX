//! Tests for migration v20260520125526: atlassian integrations

use rusqlite::Connection;

use super::v20260520125526_atlassian_integrations;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    v20260520125526_atlassian_integrations::migrate(&conn).unwrap();

    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*)
               FROM sqlite_master
              WHERE type = 'table'
                AND name = 'atlassian_integration_settings'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(exists, 1);

    let row: (i64, String, i64, i64) = conn
        .query_row(
            "SELECT enabled, validation_status, jira_available, confluence_available
               FROM atlassian_integration_settings
              WHERE id = 'default'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(row, (0, "not_configured".to_string(), 0, 0));
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260520125526_atlassian_integrations::migrate(&conn).unwrap();
    v20260520125526_atlassian_integrations::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM atlassian_integration_settings WHERE id = 'default'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}
