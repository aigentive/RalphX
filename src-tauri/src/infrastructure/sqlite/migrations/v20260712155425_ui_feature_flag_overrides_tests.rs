use rusqlite::Connection;

use super::v20260712155425_ui_feature_flag_overrides;

#[test]
fn persona_flag_override_migration_creates_singleton_row_with_null_override() {
    let conn = Connection::open_in_memory().expect("test database should open");

    v20260712155425_ui_feature_flag_overrides::migrate(&conn).expect("migration should succeed");

    let table_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'ui_feature_flag_overrides')",
            [],
            |row| row.get(0),
        )
        .expect("table lookup should succeed");
    let agent_personas_override: Option<i64> = conn
        .query_row(
            "SELECT agent_personas FROM ui_feature_flag_overrides WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("singleton row should exist");

    assert!(table_exists);
    assert_eq!(agent_personas_override, None);
}

#[test]
fn persona_flag_override_migration_is_idempotent() {
    let conn = Connection::open_in_memory().expect("test database should open");

    v20260712155425_ui_feature_flag_overrides::migrate(&conn)
        .expect("first migration should succeed");
    v20260712155425_ui_feature_flag_overrides::migrate(&conn).expect("rerun should succeed");

    let singleton_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ui_feature_flag_overrides",
            [],
            |row| row.get(0),
        )
        .expect("singleton row count should succeed");

    assert_eq!(singleton_count, 1);
}
