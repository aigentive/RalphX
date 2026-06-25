//! Tests for migration v20260612124826: provider cli management policy

use rusqlite::Connection;

use super::helpers::column_exists;
use super::v20260612124826_provider_cli_management_policy;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE agent_provider_settings (
            provider TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            is_default INTEGER NOT NULL DEFAULT 0
        );",
    )
    .unwrap();

    v20260612124826_provider_cli_management_policy::migrate(&conn).unwrap();

    assert!(column_exists(
        &conn,
        "agent_provider_settings",
        "cli_management_mode"
    ));
    assert!(column_exists(
        &conn,
        "agent_provider_settings",
        "auto_update_enabled"
    ));
}

#[test]
fn existing_rows_default_to_user_managed_without_auto_update() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE agent_provider_settings (
            provider TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            is_default INTEGER NOT NULL DEFAULT 0
        );
        INSERT INTO agent_provider_settings (provider, enabled, is_default)
        VALUES ('codex', 1, 1);",
    )
    .unwrap();

    v20260612124826_provider_cli_management_policy::migrate(&conn).unwrap();

    let row: (String, i64) = conn
        .query_row(
            "SELECT cli_management_mode, auto_update_enabled
             FROM agent_provider_settings WHERE provider = 'codex'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, ("user_managed".to_string(), 0));
}

#[test]
fn missing_provider_table_is_a_noop() {
    let conn = setup_test_db();

    v20260612124826_provider_cli_management_policy::migrate(&conn).unwrap();
}
