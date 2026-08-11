//! Tests for migration v20260718182035: add tasks enabled setting

use rusqlite::Connection;

use super::v20260718182035_add_tasks_enabled_setting;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE ideation_settings (id INTEGER PRIMARY KEY);
         INSERT INTO ideation_settings (id) VALUES (1);",
    )
    .expect("create preceding schema");
    conn
}

#[test]
fn test_migration_defaults_existing_install_to_tasks_disabled() {
    let conn = setup_test_db();
    v20260718182035_add_tasks_enabled_setting::migrate(&conn).unwrap();

    let tasks_enabled: i64 = conn
        .query_row(
            "SELECT tasks_enabled FROM ideation_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tasks_enabled, 0);
}
