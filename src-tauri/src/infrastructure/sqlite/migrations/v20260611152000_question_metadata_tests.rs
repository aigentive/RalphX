//! Tests for migration v20260611152000: pending question metadata

use rusqlite::Connection;

use super::v20260611152000_question_metadata;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE pending_questions (
            request_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            question TEXT NOT NULL,
            header TEXT,
            options TEXT NOT NULL DEFAULT '[]',
            multi_select INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'pending',
            answer_selected_options TEXT,
            answer_text TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            resolved_at TEXT
        );",
    )
    .expect("create pending_questions");
    conn
}

#[test]
fn test_migration_adds_metadata_column() {
    let conn = setup_test_db();
    v20260611152000_question_metadata::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO pending_questions (
            request_id,
            session_id,
            question,
            metadata
         ) VALUES (
            'req-1',
            'conversation-1',
            'Switch to Plan mode?',
            '{\"kind\":\"plan_mode_proposal\"}'
         )",
        [],
    )
    .unwrap();

    let metadata: Option<String> = conn
        .query_row(
            "SELECT metadata FROM pending_questions WHERE request_id = 'req-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        metadata.as_deref(),
        Some("{\"kind\":\"plan_mode_proposal\"}"),
    );
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260611152000_question_metadata::migrate(&conn).unwrap();
    v20260611152000_question_metadata::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('pending_questions')
             WHERE name = 'metadata'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 1);
}
