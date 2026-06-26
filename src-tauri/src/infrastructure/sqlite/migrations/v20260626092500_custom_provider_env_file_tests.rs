//! Tests for migration v20260626092500: custom provider env file settings

use rusqlite::Connection;

use super::helpers::column_exists;
use super::v20260626092500_custom_provider_env_file;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn migration_adds_custom_env_file_columns() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE agent_provider_settings (
            provider TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            is_default INTEGER NOT NULL DEFAULT 0
        );",
    )
    .unwrap();

    v20260626092500_custom_provider_env_file::migrate(&conn).unwrap();

    assert!(column_exists(
        &conn,
        "agent_provider_settings",
        "custom_env_file_enabled"
    ));
    assert!(column_exists(
        &conn,
        "agent_provider_settings",
        "custom_env_file_path"
    ));
}

#[test]
fn existing_rows_default_to_inactive_custom_env_file() {
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

    v20260626092500_custom_provider_env_file::migrate(&conn).unwrap();

    let row: (i64, Option<String>) = conn
        .query_row(
            "SELECT custom_env_file_enabled, custom_env_file_path
             FROM agent_provider_settings WHERE provider = 'codex'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (0, None));
}

#[test]
fn existing_custom_env_file_columns_are_preserved_on_rerun() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE agent_provider_settings (
            provider TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            is_default INTEGER NOT NULL DEFAULT 0,
            custom_env_file_enabled INTEGER NOT NULL DEFAULT 0,
            custom_env_file_path TEXT
        );
        INSERT INTO agent_provider_settings (
            provider,
            enabled,
            is_default,
            custom_env_file_enabled,
            custom_env_file_path
        )
        VALUES ('codex', 1, 1, 1, '/tmp/ralphx-custom-codex.env');",
    )
    .unwrap();

    v20260626092500_custom_provider_env_file::migrate(&conn).unwrap();

    let row: (i64, Option<String>) = conn
        .query_row(
            "SELECT custom_env_file_enabled, custom_env_file_path
             FROM agent_provider_settings WHERE provider = 'codex'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (1, Some("/tmp/ralphx-custom-codex.env".to_string())));
}

#[test]
fn missing_provider_table_is_a_noop() {
    let conn = setup_test_db();

    v20260626092500_custom_provider_env_file::migrate(&conn).unwrap();
}
