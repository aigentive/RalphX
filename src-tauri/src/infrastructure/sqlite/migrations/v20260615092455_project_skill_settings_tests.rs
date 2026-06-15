//! Tests for migration v20260615092455: project skill settings

use rusqlite::Connection;

use super::v20260615092455_project_skill_settings;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn project_skill_settings_migration_creates_default_off_table() {
    let conn = setup_test_db();
    conn.execute_batch("PRAGMA foreign_keys = ON; CREATE TABLE projects (id TEXT PRIMARY KEY);")
        .unwrap();

    v20260615092455_project_skill_settings::migrate(&conn).unwrap();

    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'project_skill_settings'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(exists, 1);

    conn.execute("INSERT INTO projects (id) VALUES ('project-1')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO project_skill_settings (project_id) VALUES ('project-1')",
        [],
    )
    .unwrap();
    let export_enabled: i64 = conn
        .query_row(
            "SELECT export_enabled FROM project_skill_settings WHERE project_id = 'project-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(export_enabled, 0);
}

#[test]
fn project_skill_settings_migration_rejects_invalid_boolean() {
    let conn = setup_test_db();
    conn.execute_batch("PRAGMA foreign_keys = ON; CREATE TABLE projects (id TEXT PRIMARY KEY);")
        .unwrap();

    v20260615092455_project_skill_settings::migrate(&conn).unwrap();

    conn.execute("INSERT INTO projects (id) VALUES ('project-1')", [])
        .unwrap();
    let invalid = conn.execute(
        "INSERT INTO project_skill_settings (project_id, export_enabled)
         VALUES ('project-1', 2)",
        [],
    );
    assert!(invalid.is_err());
}
