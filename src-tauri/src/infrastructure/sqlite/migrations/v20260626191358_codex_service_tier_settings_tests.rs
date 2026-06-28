//! Tests for migration v20260626191358: service tier metadata

use rusqlite::Connection;

use super::helpers::column_exists;
use super::v20260626191358_codex_service_tier_settings;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn migration_adds_service_tier_to_provider_settings_and_agent_runs() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE agent_provider_settings (
            provider TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            is_default INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE agent_runs (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL
        );",
    )
    .unwrap();

    v20260626191358_codex_service_tier_settings::migrate(&conn).unwrap();

    assert!(column_exists(
        &conn,
        "agent_provider_settings",
        "service_tier"
    ));
    assert!(column_exists(&conn, "agent_runs", "service_tier"));
}

#[test]
fn existing_rows_default_to_null_service_tier() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE agent_provider_settings (
            provider TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            is_default INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE agent_runs (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL
        );
        INSERT INTO agent_provider_settings (provider, enabled, is_default)
        VALUES ('codex', 1, 1);
        INSERT INTO agent_runs (id, conversation_id, status, started_at)
        VALUES ('run-1', 'conversation-1', 'running', '2026-06-26T19:13:58+00:00');",
    )
    .unwrap();

    v20260626191358_codex_service_tier_settings::migrate(&conn).unwrap();

    let provider_service_tier: Option<String> = conn
        .query_row(
            "SELECT service_tier FROM agent_provider_settings WHERE provider = 'codex'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let run_service_tier: Option<String> = conn
        .query_row(
            "SELECT service_tier FROM agent_runs WHERE id = 'run-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(provider_service_tier, None);
    assert_eq!(run_service_tier, None);
}

#[test]
fn existing_service_tier_columns_are_preserved_on_rerun() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE agent_provider_settings (
            provider TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            is_default INTEGER NOT NULL DEFAULT 0,
            service_tier TEXT
        );
        CREATE TABLE agent_runs (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL,
            service_tier TEXT
        );
        INSERT INTO agent_provider_settings (provider, enabled, is_default, service_tier)
        VALUES ('codex', 1, 1, 'fast');
        INSERT INTO agent_runs (id, conversation_id, status, started_at, service_tier)
        VALUES ('run-1', 'conversation-1', 'completed', '2026-06-26T19:13:58+00:00', 'fast');",
    )
    .unwrap();

    v20260626191358_codex_service_tier_settings::migrate(&conn).unwrap();

    let row: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT
                (SELECT service_tier FROM agent_provider_settings WHERE provider = 'codex'),
                (SELECT service_tier FROM agent_runs WHERE id = 'run-1')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (Some("fast".to_string()), Some("fast".to_string())));
}

#[test]
fn missing_tables_are_a_noop() {
    let conn = setup_test_db();

    v20260626191358_codex_service_tier_settings::migrate(&conn).unwrap();
}
