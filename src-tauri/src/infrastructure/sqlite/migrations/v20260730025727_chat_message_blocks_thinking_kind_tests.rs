//! Tests for migration v20260730025727: chat message blocks thinking kind

use rusqlite::{Connection, OptionalExtension};

use super::{
    helpers::{introduced_violations, ForeignKeyViolationCounts},
    run_migrations_through, v20260510185257_chat_message_blocks_timeline,
    v20260730000304_chat_message_blocks_created_at_index,
    v20260730025727_chat_message_blocks_thinking_kind,
};
use crate::error::AppError;
use crate::infrastructure::sqlite::open_memory_connection;

/// The migration registered immediately before this one; the orphan fixture
/// needs the real schema chain, not the minimal timeline fixture below.
const PREVIOUS_SCHEMA_VERSION: i64 = 20260730000304;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

fn migrated_connection() -> Connection {
    let conn = setup_test_db();
    // The baseline migration backfills blocks from chat_messages, so the
    // fixture needs the columns that backfill reads.
    conn.execute_batch(
        "CREATE TABLE chat_conversations (id TEXT PRIMARY KEY);
         CREATE TABLE chat_messages (
             id TEXT PRIMARY KEY,
             conversation_id TEXT,
             role TEXT,
             content TEXT,
             content_blocks TEXT,
             created_at TEXT
         );",
    )
    .unwrap();
    v20260510185257_chat_message_blocks_timeline::migrate(&conn).unwrap();
    // Runs ahead of this migration in MIGRATIONS, so the rebuild must find and
    // preserve the index it creates.
    v20260730000304_chat_message_blocks_created_at_index::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO chat_conversations (id) VALUES ('conversation')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_messages (id, conversation_id, role, content, created_at)
         VALUES ('message', 'conversation', 'assistant', '', 'now')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_message_blocks (id, conversation_id, message_id, sequence, block_index, role, kind, status, text, metadata, created_at, updated_at)
         VALUES ('existing', 'conversation', 'message', 1, 0, 'assistant', 'text', 'finalized', 'saved text', '{\"saved\":true}', 'now', 'now')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_message_block_payloads (block_id, input_json, result_json, raw_block_json, updated_at)
         VALUES ('existing', '{\"input\":true}', '{\"result\":true}', '{\"raw\":true}', 'now')",
        [],
    )
    .unwrap();
    v20260730025727_chat_message_blocks_thinking_kind::migrate(&conn).unwrap();
    conn
}

#[test]
fn preserves_rows_and_enforces_rebuilt_chat_message_block_constraints() {
    let conn = migrated_connection();

    let existing: (String, String, String) = conn
        .query_row(
            "SELECT kind, text, metadata FROM chat_message_blocks WHERE id = 'existing'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        existing,
        (
            "text".into(),
            "saved text".into(),
            "{\"saved\":true}".into()
        )
    );

    let payload: (String, String, String) = conn
        .query_row(
            "SELECT input_json, result_json, raw_block_json
             FROM chat_message_block_payloads WHERE block_id = 'existing'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        payload,
        (
            "{\"input\":true}".into(),
            "{\"result\":true}".into(),
            "{\"raw\":true}".into()
        )
    );
    assert_eq!(
        conn.query_row("PRAGMA foreign_key_check", [], |row| row.get::<_, i64>(0))
            .optional()
            .unwrap(),
        None
    );

    conn.execute(
        "INSERT INTO chat_message_blocks (id, conversation_id, message_id, sequence, block_index, role, kind, status, created_at, updated_at)
         VALUES ('thinking', 'conversation', 'message', 2, 1, 'assistant', 'thinking', 'finalized', 'now', 'now')",
        [],
    )
    .unwrap();
    assert!(conn.execute(
        "INSERT INTO chat_message_blocks (id, conversation_id, sequence, block_index, role, kind, status, created_at, updated_at)
         VALUES ('bogus', 'conversation', 3, 2, 'assistant', 'bogus', 'finalized', 'now', 'now')", [],).is_err());
    assert!(conn.execute(
        "INSERT INTO chat_message_blocks (id, conversation_id, sequence, block_index, role, kind, status, created_at, updated_at)
         VALUES ('duplicate-sequence', 'conversation', 2, 2, 'assistant', 'text', 'finalized', 'now', 'now')", [],).is_err());
    assert!(conn.execute(
        "INSERT INTO chat_message_blocks (id, conversation_id, message_id, sequence, block_index, role, kind, status, created_at, updated_at)
         VALUES ('duplicate-index', 'conversation', 'message', 3, 1, 'assistant', 'text', 'finalized', 'now', 'now')", [],).is_err());
    assert!(conn.execute(
        "INSERT INTO chat_message_block_payloads (block_id, updated_at) VALUES ('missing', 'now')", [],).is_err());
}

/// A table rebuild drops the old table's indices with it; forgetting to
/// recreate one degrades every timeline page read to a scan without failing
/// anything visibly.
#[test]
fn rebuild_recreates_every_chat_message_block_index() {
    let conn = migrated_connection();

    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND tbl_name = 'chat_message_blocks' AND name LIKE 'idx_%'
             ORDER BY name",
        )
        .unwrap();
    let indices: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    assert_eq!(
        indices,
        vec![
            "idx_chat_message_blocks_conversation_sequence".to_string(),
            // Created by v20260730000304 for the payload retention prune's
            // ORDER BY created_at + LIMIT batches; losing it here silently
            // returns that prune to a full scan per batch.
            "idx_chat_message_blocks_created_at".to_string(),
            "idx_chat_message_blocks_message".to_string(),
            "idx_chat_message_blocks_tool_call".to_string(),
        ]
    );
}

/// The refusal reaches the user through the startup failure screen, which keys
/// off the error variant. `Database(String)` there would render as the generic
/// "RalphX could not open its local workspace" with no way to act on it.
#[test]
fn refuses_with_a_typed_disk_space_error_carrying_both_measurements() {
    let error = v20260730025727_chat_message_blocks_thinking_kind::check_free_space(16_000, 3_000)
        .expect_err("preflight should refuse when the volume is short");

    assert!(
        matches!(
            error,
            AppError::InsufficientDiskSpace {
                required_bytes: 16_000,
                available_bytes: 3_000,
                ..
            }
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn allows_the_rebuild_when_free_space_exactly_meets_the_requirement() {
    v20260730025727_chat_message_blocks_thinking_kind::check_free_space(16_000, 16_000)
        .expect("equal free space should be accepted");
}

fn violation_counts(entries: &[(&str, &str, i64, i64)]) -> ForeignKeyViolationCounts {
    entries
        .iter()
        .map(|(table, parent, fkid, count)| {
            (((*table).to_string(), (*parent).to_string(), *fkid), *count)
        })
        .collect()
}

/// Live databases carry orphan rows this rebuild neither created nor cleans up:
/// foreign keys are enforced by default, but migrations that rewrite tables turn
/// them off, so deletes inside those windows leave children behind. A
/// database-wide integrity check counted those pre-existing violations as rebuild
/// damage, so the migration failed, `AppState` initialization aborted, and the app
/// refused to open its local workspace on every launch with no way to recover.
#[test]
fn migration_ignores_preexisting_unrelated_foreign_key_violations() {
    let conn = open_memory_connection().expect("create memory db");
    run_migrations_through(&conn, PREVIOUS_SCHEMA_VERSION).expect("run prior migrations");
    conn.execute("PRAGMA foreign_keys = OFF", []).unwrap();
    // The production shape: a sync record whose parent link was purged while
    // foreign keys were off, in a table this migration never touches.
    conn.execute(
        "INSERT INTO external_issue_sync_records
            (id, link_id, sync_kind, idempotency_key, status)
         VALUES ('sync-orphan', 'missing-link', 'push', 'idem-orphan', 'pending')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('project-1', 'Project', '/tmp/project')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id)
         VALUES ('conversation-1', 'project', 'project-1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_message_blocks
            (id, conversation_id, sequence, block_index, role, kind, status, text, created_at, updated_at)
         VALUES ('block-kept', 'conversation-1', 1, 0, 'assistant', 'text', 'finalized', 'saved text', 'now', 'now')",
        [],
    )
    .unwrap();
    // A pre-existing orphan inside the rebuilt table itself: the copy carries it
    // across, so the baseline diff must key on the constraint rather than the
    // rowids the rebuild renumbers.
    conn.execute(
        "INSERT INTO chat_message_blocks
            (id, conversation_id, message_id, sequence, block_index, role, kind, status, created_at, updated_at)
         VALUES ('block-orphan', 'conversation-1', 'missing-message', 2, 0, 'assistant', 'text', 'finalized', 'now', 'now')",
        [],
    )
    .unwrap();

    v20260730025727_chat_message_blocks_thinking_kind::migrate(&conn)
        .expect("pre-existing unrelated violations must not block the rebuild");

    assert_eq!(
        conn.query_row(
            "SELECT text FROM chat_message_blocks WHERE id = 'block-kept'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "saved text"
    );
    conn.execute(
        "INSERT INTO chat_message_blocks
            (id, conversation_id, sequence, block_index, role, kind, status, created_at, updated_at)
         VALUES ('block-thinking', 'conversation-1', 3, 1, 'assistant', 'thinking', 'finalized', 'now', 'now')",
        [],
    )
    .expect("the rebuilt CHECK constraint must accept the thinking kind");
    // The migration must not silently repair unrelated data it does not own.
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM external_issue_sync_records WHERE id = 'sync-orphan'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM chat_message_blocks WHERE id = 'block-orphan'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
}

#[test]
fn introduced_violations_reports_only_the_excess_over_the_baseline() {
    let baseline =
        violation_counts(&[("external_issue_sync_records", "external_issue_links", 0, 4)]);
    let after = violation_counts(&[("external_issue_sync_records", "external_issue_links", 0, 7)]);

    assert_eq!(
        introduced_violations(&baseline, &after),
        vec![(
            "external_issue_sync_records".to_string(),
            "external_issue_links".to_string(),
            3
        )]
    );
}

#[test]
fn introduced_violations_reports_a_constraint_absent_from_the_baseline() {
    let baseline = ForeignKeyViolationCounts::new();
    let after = violation_counts(&[("chat_message_blocks", "chat_conversations", 1, 2)]);

    assert_eq!(
        introduced_violations(&baseline, &after),
        vec![(
            "chat_message_blocks".to_string(),
            "chat_conversations".to_string(),
            2
        )]
    );
}

#[test]
fn introduced_violations_ignores_unchanged_and_disappearing_baseline_entries() {
    let baseline = violation_counts(&[
        ("external_issue_sync_records", "external_issue_links", 0, 4),
        ("chat_message_blocks", "chat_messages", 0, 2),
    ]);
    let after = violation_counts(&[("external_issue_sync_records", "external_issue_links", 0, 4)]);

    assert!(introduced_violations(&baseline, &after).is_empty());
}
