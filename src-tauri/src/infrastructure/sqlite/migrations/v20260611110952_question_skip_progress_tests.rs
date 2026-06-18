//! Tests for migration v20260611110952: question skip progress

use rusqlite::Connection;

use super::v20260611110952_question_skip_progress;

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
fn test_migration_adds_skip_and_progress_columns() {
    let conn = setup_test_db();
    v20260611110952_question_skip_progress::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO pending_questions (
            request_id,
            session_id,
            question,
            batch_index,
            batch_total,
            answer_skipped
         ) VALUES ('req-1', 'session-1', 'Which path?', 1, 3, 1)",
        [],
    )
    .unwrap();

    let row = conn
        .query_row(
            "SELECT allow_skip, batch_index, batch_total, answer_skipped
             FROM pending_questions WHERE request_id = 'req-1'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(row, (1, Some(1), Some(3), 1));
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260611110952_question_skip_progress::migrate(&conn).unwrap();
    v20260611110952_question_skip_progress::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('pending_questions')
             WHERE name IN ('allow_skip', 'batch_index', 'batch_total', 'answer_skipped')",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 4);
}
