use rusqlite::Connection;

use super::helpers;
use super::v20260723100604_app_state_update_channel;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("create in-memory database");
    conn.execute_batch(
        "CREATE TABLE app_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            active_project_id TEXT DEFAULT NULL,
            updated_at TEXT NOT NULL
        );
        INSERT INTO app_state (id, active_project_id, updated_at)
        VALUES (1, NULL, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'));",
    )
    .expect("create app state schema");
    conn
}

#[test]
fn adds_stable_update_channel_for_existing_app_state() {
    let conn = setup_test_db();

    v20260723100604_app_state_update_channel::migrate(&conn).unwrap();

    assert!(helpers::column_exists(&conn, "app_state", "update_channel"));
    let channel: String = conn
        .query_row(
            "SELECT update_channel FROM app_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(channel, "stable");
}

#[test]
fn update_channel_migration_is_idempotent() {
    let conn = setup_test_db();

    v20260723100604_app_state_update_channel::migrate(&conn).unwrap();
    v20260723100604_app_state_update_channel::migrate(&conn).unwrap();

    assert!(helpers::column_exists(&conn, "app_state", "update_channel"));
}
