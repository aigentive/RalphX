use crate::infrastructure::sqlite::migrations::helpers::{column_exists, index_exists};
use crate::infrastructure::sqlite::migrations::v20260407073000_provider_harness_metadata;
use crate::infrastructure::sqlite::{open_memory_connection, run_migrations};

fn insert_conversation(conn: &rusqlite::Connection, id: &str) {
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id)
         VALUES (?1, 'project', 'project-1')",
        [id],
    )
    .unwrap();
}

#[test]
fn test_provider_harness_metadata_columns_exist() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    for column in ["provider_session_id", "provider_harness"] {
        assert!(column_exists(&conn, "chat_conversations", column));
    }

    for column in [
        "harness",
        "provider_session_id",
        "logical_model",
        "effective_model_id",
        "logical_effort",
        "effective_effort",
        "approval_policy",
        "sandbox_mode",
    ] {
        assert!(column_exists(&conn, "agent_runs", column));
    }
}

#[test]
fn test_provider_harness_metadata_defaults_to_null() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    insert_conversation(&conn, "conv-1");
    conn.execute(
        "INSERT INTO agent_runs (id, conversation_id, status, started_at)
         VALUES ('run-1', 'conv-1', 'running', '2026-01-01T00:00:00+00:00')",
        [],
    )
    .unwrap();

    let conversation_fields: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT provider_session_id, provider_harness
             FROM chat_conversations
             WHERE id = 'conv-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(conversation_fields, (None, None));

    let run_fields: (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT harness, provider_session_id, logical_model, effective_model_id,
                    logical_effort, effective_effort, approval_policy, sandbox_mode
             FROM agent_runs
             WHERE id = 'run-1'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(run_fields, (None, None, None, None, None, None, None, None));
}

#[test]
fn test_provider_harness_metadata_indexes_exist() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    assert!(index_exists(
        &conn,
        "idx_chat_conversations_provider_session"
    ));
    assert!(index_exists(&conn, "idx_agent_runs_provider_session"));
}

#[test]
fn test_provider_harness_metadata_is_queryable() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    insert_conversation(&conn, "conv-1");
    conn.execute(
        "UPDATE chat_conversations
         SET provider_session_id = 'session-123',
             provider_harness = 'codex'
         WHERE id = 'conv-1'",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_runs (
            id, conversation_id, status, started_at, harness, provider_session_id,
            logical_model, effective_model_id, logical_effort, effective_effort,
            approval_policy, sandbox_mode
         ) VALUES (
            'run-1', 'conv-1', 'completed', '2026-01-01T00:00:00+00:00',
            'codex', 'session-123', 'gpt-5.4', 'gpt-5.4', 'xhigh', 'high',
            'on-request', 'workspace-write'
         )",
        [],
    )
    .unwrap();

    let conversation_id: String = conn
        .query_row(
            "SELECT id FROM chat_conversations
             WHERE provider_harness = 'codex' AND provider_session_id = 'session-123'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(conversation_id, "conv-1");

    let run_fields: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT harness, provider_session_id, logical_model, effective_model_id,
                    logical_effort, effective_effort, approval_policy, sandbox_mode
             FROM agent_runs
             WHERE harness = 'codex' AND provider_session_id = 'session-123'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        run_fields,
        (
            "codex".to_string(),
            "session-123".to_string(),
            "gpt-5.4".to_string(),
            "gpt-5.4".to_string(),
            "xhigh".to_string(),
            "high".to_string(),
            "on-request".to_string(),
            "workspace-write".to_string(),
        )
    );
}

#[test]
fn test_provider_harness_metadata_migration_is_idempotent() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    v20260407073000_provider_harness_metadata::migrate(&conn).unwrap();
}
