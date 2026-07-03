//! Tests for migration v20260630123000: Workspace Review policy setting

use rusqlite::Connection;

use super::helpers;
use super::v20260630123000_workspace_review_policy_setting;

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
         VALUES (1, '2026-06-30T12:30:00+00:00')",
        [],
    )
    .expect("review settings row should be seeded");
    conn
}

#[test]
fn migration_adds_workspace_review_required_default() {
    let conn = setup_test_db();
    v20260630123000_workspace_review_policy_setting::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "review_settings",
        "require_workspace_review"
    ));

    let enabled: i64 = conn
        .query_row(
            "SELECT require_workspace_review FROM review_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("workspace review setting should be readable");
    assert_eq!(enabled, 1);
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();
    v20260630123000_workspace_review_policy_setting::migrate(&conn).unwrap();
    v20260630123000_workspace_review_policy_setting::migrate(&conn).unwrap();

    let enabled: i64 = conn
        .query_row(
            "SELECT require_workspace_review FROM review_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("workspace review setting should remain readable");
    assert_eq!(enabled, 1);
}
