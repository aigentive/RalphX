//! Tests for migration v20260727161131: remote host settings

use rusqlite::Connection;

use super::{helpers, v20260727161131_remote_host_settings};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("in-memory database should open")
}

#[test]
fn migration_creates_the_remote_host_settings_singleton_schema() {
    let conn = setup_test_db();

    v20260727161131_remote_host_settings::migrate(&conn)
        .expect("migration should create remote host settings");

    assert!(helpers::table_exists(&conn, "remote_host_settings"));
    for column in ["enabled", "exposure_mode", "port", "environment_id"] {
        assert!(
            helpers::column_exists(&conn, "remote_host_settings", column),
            "remote host settings should contain {column}"
        );
    }
    let row_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM remote_host_settings", [], |row| {
            row.get(0)
        })
        .expect("singleton table should be queryable");
    assert_eq!(row_count, 0, "migration must not enable or seed host mode");
}

#[test]
fn migration_enforces_one_valid_remote_host_settings_row() {
    let conn = setup_test_db();
    v20260727161131_remote_host_settings::migrate(&conn)
        .expect("migration should create remote host settings");

    conn.execute(
        "INSERT INTO remote_host_settings (id, enabled, exposure_mode, port, environment_id)
         VALUES (1, 0, 'serve', 3849, '8d3d6a07-8e85-4e91-97ce-915fc038fdb2')",
        [],
    )
    .expect("singleton row should accept the default valid values");

    assert!(conn
        .execute(
            "INSERT INTO remote_host_settings (id, enabled, exposure_mode, port, environment_id)
             VALUES (2, 0, 'serve', 3849, 'b3a3d3c4-802a-4a02-b4c7-6b2c7aa0fd4c')",
            [],
        )
        .is_err());
    assert!(conn
        .execute(
            "UPDATE remote_host_settings SET exposure_mode = 'lan' WHERE id = 1",
            [],
        )
        .is_err());
}

#[test]
fn migration_is_idempotent_without_seeding_a_row() {
    let conn = setup_test_db();

    v20260727161131_remote_host_settings::migrate(&conn).expect("first migration should succeed");
    v20260727161131_remote_host_settings::migrate(&conn)
        .expect("second migration should remain safe");

    let row_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM remote_host_settings", [], |row| {
            row.get(0)
        })
        .expect("singleton table should be queryable");
    assert_eq!(row_count, 0);
}
