//! Tests for migration v20260802174000: persisted Workspace Review fixer cycle cap.

use rusqlite::Connection;

use super::helpers;
use super::v20260802174000_workspace_review_fixer_cycle_cap;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("create migration test database");
    conn.execute_batch(
        "CREATE TABLE agent_workspace_review_monitors (conversation_id TEXT PRIMARY KEY);
         CREATE TABLE review_settings (id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1));
         INSERT INTO review_settings (id) VALUES (1);",
    )
    .expect("seed legacy-shaped migration database");
    conn
}

#[test]
fn migration_adds_cycle_defaults_to_legacy_tables() {
    let conn = setup_test_db();

    v20260802174000_workspace_review_fixer_cycle_cap::migrate(&conn)
        .expect("migration should succeed");

    assert!(helpers::column_exists(
        &conn,
        "agent_workspace_review_monitors",
        "review_fixer_cycle_count"
    ));
    assert!(helpers::column_exists(
        &conn,
        "review_settings",
        "workspace_review_fixer_cycle_cap"
    ));
    conn.execute(
        "INSERT INTO agent_workspace_review_monitors (conversation_id) VALUES (?1)",
        ["monitor-default-count"],
    )
    .expect("monitor with migration default should insert");
    let cycle_count: i64 = conn
        .query_row(
            "SELECT review_fixer_cycle_count
             FROM agent_workspace_review_monitors
             WHERE conversation_id = 'monitor-default-count'",
            [],
            |row| row.get(0),
        )
        .expect("monitor cycle default should be readable");
    assert_eq!(cycle_count, 0);
    let cap: i64 = conn
        .query_row(
            "SELECT workspace_review_fixer_cycle_cap FROM review_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("cycle cap default should be readable");
    assert_eq!(cap, 3);
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();

    v20260802174000_workspace_review_fixer_cycle_cap::migrate(&conn)
        .expect("first migration should succeed");
    v20260802174000_workspace_review_fixer_cycle_cap::migrate(&conn)
        .expect("second migration should succeed");

    let cap: i64 = conn
        .query_row(
            "SELECT workspace_review_fixer_cycle_cap FROM review_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("cycle cap should remain readable");
    assert_eq!(cap, 3);
}
