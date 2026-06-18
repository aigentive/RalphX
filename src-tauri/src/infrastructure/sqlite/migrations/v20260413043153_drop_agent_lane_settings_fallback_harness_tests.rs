//! Tests for migration v20260413043153: drop agent lane settings fallback harness

use rusqlite::Connection;

use super::v20260413043153_drop_agent_lane_settings_fallback_harness;
use crate::infrastructure::sqlite::migrations::helpers::{column_exists, index_exists};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_drops_fallback_harness_column_and_preserves_rows() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE agent_lane_settings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scope_type TEXT NOT NULL,
            scope_id TEXT,
            lane TEXT NOT NULL,
            harness TEXT NOT NULL,
            model TEXT,
            effort TEXT,
            approval_policy TEXT,
            sandbox_mode TEXT,
            fallback_harness TEXT,
            updated_at TEXT NOT NULL
         );
         CREATE UNIQUE INDEX idx_agent_lane_settings_scope_lane
            ON agent_lane_settings(scope_type, scope_id, lane);
         CREATE INDEX idx_agent_lane_settings_scope
            ON agent_lane_settings(scope_type, scope_id);
         INSERT INTO agent_lane_settings (
            scope_type, scope_id, lane, harness, model, effort,
            approval_policy, sandbox_mode, fallback_harness, updated_at
         ) VALUES (
            'global', NULL, 'ideation_primary', 'codex', 'gpt-5.4', 'xhigh',
            'never', 'danger-full-access', 'claude', '2026-04-13T00:00:00+00:00'
         );",
    )
    .unwrap();

    v20260413043153_drop_agent_lane_settings_fallback_harness::migrate(&conn).unwrap();

    assert!(!column_exists(
        &conn,
        "agent_lane_settings",
        "fallback_harness"
    ));
    assert!(index_exists(&conn, "idx_agent_lane_settings_scope_lane"));
    assert!(index_exists(&conn, "idx_agent_lane_settings_scope"));

    let row: (String, String, String) = conn
        .query_row(
            "SELECT harness, model, updated_at FROM agent_lane_settings WHERE lane = 'ideation_primary'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        (
            "codex".to_string(),
            "gpt-5.4".to_string(),
            "2026-04-13T00:00:00+00:00".to_string()
        )
    );
}

#[test]
fn test_migration_is_noop_when_column_already_absent() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE agent_lane_settings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scope_type TEXT NOT NULL,
            scope_id TEXT,
            lane TEXT NOT NULL,
            harness TEXT NOT NULL,
            model TEXT,
            effort TEXT,
            approval_policy TEXT,
            sandbox_mode TEXT,
            updated_at TEXT NOT NULL
         );",
    )
    .unwrap();

    v20260413043153_drop_agent_lane_settings_fallback_harness::migrate(&conn).unwrap();

    assert!(!column_exists(
        &conn,
        "agent_lane_settings",
        "fallback_harness"
    ));
}
