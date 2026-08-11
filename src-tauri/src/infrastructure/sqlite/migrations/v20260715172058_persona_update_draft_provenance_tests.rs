//! Tests for migration v20260715172058: persona update draft provenance

use rusqlite::Connection;

use super::{v20260711151804_personas, v20260715172058_persona_update_draft_provenance};

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE chat_conversations (id TEXT PRIMARY KEY);",
    )
    .expect("chat conversation fixture");
    v20260711151804_personas::migrate(&conn).expect("persona baseline migration");
    conn
}

#[test]
fn migration_adds_provenance_and_builder_binding_columns_idempotently() {
    let conn = setup_test_db();
    v20260715172058_persona_update_draft_provenance::migrate(&conn)
        .expect("migration should succeed");
    v20260715172058_persona_update_draft_provenance::migrate(&conn)
        .expect("migration rerun should succeed");

    for (table, column) in [
        ("personas", "source_persona_id"),
        ("personas", "source_content_hash"),
        ("chat_conversations", "builder_draft_id"),
    ] {
        let count: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
                [column],
                |row| row.get(0),
            )
            .expect("column count");
        assert_eq!(count, 1, "{table}.{column} should exist once");
    }

    let binding_index_exists: bool = conn
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_chat_conversations_builder_draft_id'
             )",
            [],
            |row| row.get(0),
        )
        .expect("builder binding index lookup");
    assert!(binding_index_exists);
}

#[test]
fn migration_allows_seeded_drafts_to_share_an_active_slug_but_rejects_two_active_rows() {
    let conn = setup_test_db();
    v20260715172058_persona_update_draft_provenance::migrate(&conn)
        .expect("migration should succeed");

    conn.execute(
        "INSERT INTO personas (
             id, slug, name, content, status, content_hash, created_at, updated_at
         ) VALUES ('active', 'reviewer', 'Reviewer', 'active', 'active', 'hash-a', 'now', 'now')",
        [],
    )
    .expect("active persona");
    conn.execute(
        "INSERT INTO personas (
             id, slug, name, content, status, content_hash,
             source_persona_id, source_content_hash, created_at, updated_at
         ) VALUES (
             'draft-one', 'reviewer', 'Reviewer update', 'draft', 'draft', 'hash-d1',
             'active', 'hash-a', 'now', 'now'
         )",
        [],
    )
    .expect("first seeded draft may share active slug");
    conn.execute(
        "INSERT INTO personas (
             id, slug, name, content, status, content_hash,
             source_persona_id, source_content_hash, created_at, updated_at
         ) VALUES (
             'draft-two', 'reviewer', 'Reviewer update two', 'draft', 'draft', 'hash-d2',
             'active', 'hash-a', 'now', 'now'
         )",
        [],
    )
    .expect("second seeded draft may share active slug");

    let duplicate_active = conn.execute(
        "INSERT INTO personas (
             id, slug, name, content, status, content_hash, created_at, updated_at
         ) VALUES ('active-two', 'reviewer', 'Duplicate', 'active', 'active', 'hash-b', 'now', 'now')",
        [],
    );
    assert!(duplicate_active.is_err());
}

#[test]
fn deleting_a_bound_draft_clears_the_conversation_binding() {
    let conn = setup_test_db();
    v20260715172058_persona_update_draft_provenance::migrate(&conn)
        .expect("migration should succeed");
    conn.execute(
        "INSERT INTO personas (
             id, slug, name, content, status, content_hash, created_at, updated_at
         ) VALUES ('draft', 'reviewer', 'Reviewer', 'draft', 'draft', 'hash', 'now', 'now')",
        [],
    )
    .expect("draft persona");
    conn.execute(
        "INSERT INTO chat_conversations (id, builder_draft_id) VALUES ('conversation', 'draft')",
        [],
    )
    .expect("bound conversation");

    conn.execute("DELETE FROM personas WHERE id = 'draft'", [])
        .expect("delete draft");

    let builder_draft_id: Option<String> = conn
        .query_row(
            "SELECT builder_draft_id FROM chat_conversations WHERE id = 'conversation'",
            [],
            |row| row.get(0),
        )
        .expect("conversation binding");
    assert_eq!(builder_draft_id, None);
}
