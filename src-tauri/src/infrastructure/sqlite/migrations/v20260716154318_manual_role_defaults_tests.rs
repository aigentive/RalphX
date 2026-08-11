//! Tests for migration v20260716154318: manual role defaults

use rusqlite::Connection;

use super::v20260716154318_manual_role_defaults;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn creates_unseeded_manual_role_defaults_table() {
    let conn = setup_test_db();
    v20260716154318_manual_role_defaults::migrate(&conn).unwrap();

    let columns = conn
        .prepare("PRAGMA table_info(manual_role_defaults)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        columns,
        vec![
            "id",
            "scope_type",
            "scope_id",
            "role",
            "value_json",
            "updated_at"
        ]
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM manual_role_defaults", [], |row| row
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}

#[test]
fn scope_and_role_are_unique_and_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260716154318_manual_role_defaults::migrate(&conn).unwrap();
    v20260716154318_manual_role_defaults::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO manual_role_defaults (scope_type, scope_id, role, value_json)
         VALUES ('project', 'project-1', 'workspace_edit', '{}')",
        [],
    )
    .unwrap();
    let duplicate = conn.execute(
        "INSERT INTO manual_role_defaults (scope_type, scope_id, role, value_json)
         VALUES ('project', 'project-1', 'workspace_edit', '{}')",
        [],
    );
    assert!(duplicate.is_err());
}
