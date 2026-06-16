//! Tests for migration v20260616182951: linear webhook reconciliation

use rusqlite::Connection;

use super::v20260616182951_linear_webhook_reconciliation;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    v20260616182951_linear_webhook_reconciliation::migrate(&conn).unwrap();
}

#[test]
fn creates_linear_webhook_reconciliation_tables() {
    let conn = setup_test_db();
    v20260616182951_linear_webhook_reconciliation::migrate(&conn).unwrap();

    for table in [
        "linear_webhook_config",
        "linear_webhook_deliveries",
        "external_issue_links",
        "external_issue_sync_events",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "{table} should exist");
    }
}

#[test]
fn stores_only_linear_webhook_secret_ref_seed_row() {
    let conn = setup_test_db();
    v20260616182951_linear_webhook_reconciliation::migrate(&conn).unwrap();

    let row: (String, i64, Option<String>) = conn
        .query_row(
            "SELECT id, enabled, signing_secret_ref FROM linear_webhook_config",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(row, ("default".to_string(), 0, None));
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();
    v20260616182951_linear_webhook_reconciliation::migrate(&conn).unwrap();
    v20260616182951_linear_webhook_reconciliation::migrate(&conn).unwrap();

    let config_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM linear_webhook_config", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(config_rows, 1);
}
