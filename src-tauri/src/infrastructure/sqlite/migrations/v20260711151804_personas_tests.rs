//! Tests for migration v20260711151804: personas

use rusqlite::Connection;

use super::v20260711151804_personas;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch("CREATE TABLE chat_conversations (id TEXT PRIMARY KEY);")
        .expect("chat_conversations fixture should be created");
    conn
}

#[test]
fn migration_creates_personas_table_index_and_binding_column() {
    let conn = setup_test_db();

    v20260711151804_personas::migrate(&conn).expect("migration should succeed");

    let personas_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'personas')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let binding_index_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = 'idx_chat_conversations_persona_id')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let persona_id_exists: bool = conn
        .prepare("PRAGMA table_info(chat_conversations)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .iter()
        .any(|column| column == "persona_id");

    assert!(personas_exists);
    assert!(binding_index_exists);
    assert!(persona_id_exists);
}

#[test]
fn migration_is_idempotent_on_rerun() {
    let conn = setup_test_db();

    v20260711151804_personas::migrate(&conn).expect("first migration should succeed");
    v20260711151804_personas::migrate(&conn).expect("rerun should succeed");

    let persona_id_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('chat_conversations') WHERE name = 'persona_id'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persona_id_count, 1);
}

#[test]
fn migration_creates_partial_unique_index_slug_live() {
    let conn = setup_test_db();
    v20260711151804_personas::migrate(&conn).expect("migration should succeed");

    conn.execute(
        "INSERT INTO personas (id, slug, name, content, content_hash, created_at, updated_at) VALUES ('active', 'reviewer', 'Reviewer', 'content', 'hash', 'now', 'now')",
        [],
    )
    .unwrap();
    let duplicate = conn.execute(
        "INSERT INTO personas (id, slug, name, content, content_hash, created_at, updated_at) VALUES ('draft', 'reviewer', 'Draft reviewer', 'content', 'hash', 'now', 'now')",
        [],
    );
    conn.execute(
        "UPDATE personas SET status = 'archived' WHERE id = 'active'",
        [],
    )
    .unwrap();
    let reused = conn.execute(
        "INSERT INTO personas (id, slug, name, content, content_hash, created_at, updated_at) VALUES ('replacement', 'reviewer', 'Replacement', 'content', 'hash', 'now', 'now')",
        [],
    );

    assert!(duplicate.is_err());
    assert!(reused.is_ok());
}
