//! Tests for migration v20260625115000: custom provider binary settings

use rusqlite::Connection;

use super::helpers::column_exists;
use super::v20260625115000_custom_provider_binary;

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

    v20260625115000_custom_provider_binary::migrate(&conn).unwrap();

    assert!(column_exists(
        &conn,
        "agent_provider_settings",
        "custom_binary_enabled"
    ));
    assert!(column_exists(
        &conn,
        "agent_provider_settings",
        "custom_binary_path"
    ));
}

#[test]
fn existing_rows_default_to_inactive_custom_binary() {
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

    v20260625115000_custom_provider_binary::migrate(&conn).unwrap();

    let row: (i64, Option<String>) = conn
        .query_row(
            "SELECT custom_binary_enabled, custom_binary_path
             FROM agent_provider_settings WHERE provider = 'codex'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (0, None));
}

#[test]
fn existing_custom_binary_columns_are_preserved_on_rerun() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE agent_provider_settings (
            provider TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            is_default INTEGER NOT NULL DEFAULT 0,
            custom_binary_enabled INTEGER NOT NULL DEFAULT 0,
            custom_binary_path TEXT
        );
        INSERT INTO agent_provider_settings (
            provider,
            enabled,
            is_default,
            custom_binary_enabled,
            custom_binary_path
        )
        VALUES ('codex', 1, 1, 1, '/opt/custom/codex-wrapper');",
    )
    .unwrap();

    v20260625115000_custom_provider_binary::migrate(&conn).unwrap();

    let row: (i64, Option<String>) = conn
        .query_row(
            "SELECT custom_binary_enabled, custom_binary_path
             FROM agent_provider_settings WHERE provider = 'codex'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (1, Some("/opt/custom/codex-wrapper".to_string())));
}

#[test]
fn missing_provider_table_is_a_noop() {
    let conn = setup_test_db();

    v20260625115000_custom_provider_binary::migrate(&conn).unwrap();
}
