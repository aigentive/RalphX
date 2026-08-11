//! Tests for migration v20260708130511: workspace review autofix setting

use rusqlite::Connection;

use super::helpers;
use super::v20260708130511_workspace_review_autofix_setting;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute(
        "CREATE TABLE review_settings (
            id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
            updated_at TEXT NOT NULL
        )",
        [],
    )
    .expect("review settings table should be created");
    conn.execute(
        "INSERT INTO review_settings (id, updated_at)
         VALUES (1, '2026-07-08T13:05:11+00:00')",
        [],
    )
    .expect("review settings row should be seeded");
    conn
}

#[test]
fn migration_adds_workspace_review_autofix_default() {
    let conn = setup_test_db();
    v20260708130511_workspace_review_autofix_setting::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "review_settings",
        "autofix_workspace_review_blocking_findings"
    ));

    let enabled: i64 = conn
        .query_row(
            "SELECT autofix_workspace_review_blocking_findings FROM review_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("workspace review autofix setting should be readable");
    assert_eq!(enabled, 1);
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();
    v20260708130511_workspace_review_autofix_setting::migrate(&conn).unwrap();
    v20260708130511_workspace_review_autofix_setting::migrate(&conn).unwrap();

    let enabled: i64 = conn
        .query_row(
            "SELECT autofix_workspace_review_blocking_findings FROM review_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("workspace review autofix setting should remain readable");
    assert_eq!(enabled, 1);
}
