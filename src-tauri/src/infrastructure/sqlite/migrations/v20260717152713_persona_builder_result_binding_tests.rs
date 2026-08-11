//! Tests for migration v20260717152713: persona builder result binding

use rusqlite::{Connection, OptionalExtension};

use super::{run_migrations_through, v20260717152713_persona_builder_result_binding};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

fn seed_persona(conn: &Connection, id: &str, status: &str) {
    conn.execute(
        "INSERT INTO personas (
            id, slug, name, description, content, status, version, content_hash,
            source_json, created_at, updated_at
         ) VALUES (?1, ?1, ?1, '', 'content', ?2, 1, 'hash', '{}',
                   '2026-07-17T00:00:00+00:00', '2026-07-17T00:00:00+00:00')",
        rusqlite::params![id, status],
    )
    .expect("persona fixture should seed");
}

#[test]
fn migration_moves_only_non_draft_builder_bindings_to_result_personas() {
    let conn = setup_test_db();
    run_migrations_through(&conn, 20260716204027).expect("baseline migrations should succeed");
    seed_persona(&conn, "draft-persona", "draft");
    seed_persona(&conn, "active-persona", "active");
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, builder_draft_id)
         VALUES ('draft-conversation', 'project', 'project', 'draft-persona'),
                ('active-conversation', 'project', 'project', 'active-persona')",
        [],
    )
    .expect("conversation fixtures should seed");

    v20260717152713_persona_builder_result_binding::migrate(&conn)
        .expect("binding migration should succeed");

    let draft_binding: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT builder_draft_id, builder_result_persona_id
             FROM chat_conversations WHERE id = 'draft-conversation'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("draft binding should load");
    assert_eq!(draft_binding, (Some("draft-persona".to_string()), None));
    let active_binding: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT builder_draft_id, builder_result_persona_id
             FROM chat_conversations WHERE id = 'active-conversation'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("active binding should load");
    assert_eq!(active_binding, (None, Some("active-persona".to_string())));

    let index_exists = conn
        .query_row(
            "SELECT 1 FROM sqlite_master
             WHERE type = 'index'
               AND name = 'idx_chat_conversations_builder_result_persona_id'",
            [],
            |_| Ok(()),
        )
        .optional()
        .expect("index lookup should succeed")
        .is_some();
    assert!(index_exists);
}
