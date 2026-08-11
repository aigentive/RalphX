//! Tests for migration v20260716170840: persona project scope

use rusqlite::{Connection, OptionalExtension};

use super::{run_migrations_through, v20260716170840_persona_project_scope};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

fn index_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1",
        [name],
        |_| Ok(()),
    )
    .optional()
    .expect("index lookup should succeed")
    .is_some()
}

#[test]
fn fresh_database_has_nullable_project_scope_and_scoped_active_slug_index() {
    let conn = setup_test_db();
    run_migrations_through(&conn, 20260716170840).expect("fresh migrations should succeed");

    let project_id: Option<String> = conn
        .query_row("SELECT project_id FROM personas LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()
        .expect("project scope query should succeed")
        .flatten();
    assert_eq!(project_id, None);
    assert!(!index_exists(&conn, "idx_personas_slug_live"));
    assert!(index_exists(&conn, "personas_active_slug_scoped"));
    assert!(index_exists(&conn, "idx_personas_project_id"));
}

#[test]
fn upgrade_preserves_existing_personas_as_global_and_replaces_old_index() {
    let conn = setup_test_db();
    run_migrations_through(&conn, 20260715172058).expect("pre-scope migrations should succeed");
    conn.execute(
        "INSERT INTO personas (
            id, slug, name, description, content, status, version, content_hash,
            source_json, created_at, updated_at
         ) VALUES ('existing', 'existing', 'Existing', 'Existing', 'body', 'active', 1,
                   'hash', '{}', '2026-07-16T00:00:00+00:00', '2026-07-16T00:00:00+00:00')",
        [],
    )
    .expect("existing persona should seed");
    assert!(index_exists(&conn, "idx_personas_slug_live"));

    v20260716170840_persona_project_scope::migrate(&conn).expect("scope migration should succeed");

    let project_id: Option<String> = conn
        .query_row(
            "SELECT project_id FROM personas WHERE id = 'existing'",
            [],
            |row| row.get(0),
        )
        .expect("existing persona should remain");
    assert_eq!(project_id, None);
    assert!(!index_exists(&conn, "idx_personas_slug_live"));
    assert!(index_exists(&conn, "personas_active_slug_scoped"));
}
