//! Tests for migration v20260710201548: notification settings

use rusqlite::Connection;

use super::v20260710201548_notification_settings;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    v20260710201548_notification_settings::migrate(&conn).unwrap();

    let columns = conn
        .prepare("PRAGMA table_info(notification_settings)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(columns, vec!["id", "settings_json", "updated_at"]);
}
