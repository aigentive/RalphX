//! Tests for migration v20260806071104: agent workspace repair effect failed completed at

use rusqlite::params;

use super::{get_schema_version, run_migrations, run_migrations_through, SCHEMA_VERSION};
use crate::infrastructure::sqlite::connection::open_memory_connection;

const PREVIOUS_SCHEMA_VERSION: i64 = 20260804125852;

fn seed_attempt(conn: &rusqlite::Connection, conversation_id: &str, attempt_id: &str) {
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id)
         VALUES (?1, 'project', 'project-1')",
        [conversation_id],
    )
    .expect("seed conversation");
    conn.execute(
        "INSERT INTO agent_conversation_workspaces (
            conversation_id, project_id, mode, base_ref_kind, base_ref, branch_name,
            worktree_path, created_at, updated_at
         ) VALUES (
            ?1, 'project-1', 'edit', 'project_default', 'main',
            'ralphx/project/agent-conversation-1', '/workspace',
            '2026-08-06T00:00:00+00:00', '2026-08-06T00:00:00+00:00'
         )",
        [conversation_id],
    )
    .expect("seed workspace");
    conn.execute(
        "INSERT INTO agent_workspace_repair_attempts (
            id, conversation_id, generation, source, phase, continuation,
            target_base_ref, review_required, auto_publish_enabled, auto_merge_desired,
            created_at, updated_at
         ) VALUES (
            ?1, ?2, 1, 'base_update', 'continuing', 'update_only',
            'origin/main', 0, 0, 0,
            '2026-08-06T00:00:00+00:00', '2026-08-06T00:00:00+00:00'
         )",
        params![attempt_id, conversation_id],
    )
    .expect("seed repair attempt");
}

#[test]
fn fresh_schema_allows_failed_effect_with_completed_at_and_no_receipt() {
    let conn = open_memory_connection().expect("open memory database");
    run_migrations(&conn).expect("run fresh migrations");
    seed_attempt(&conn, "conversation-1", "repair-1");

    assert_eq!(get_schema_version(&conn).unwrap(), SCHEMA_VERSION);

    conn.execute(
        "INSERT INTO agent_workspace_repair_effects (
            id, attempt_id, kind, status, idempotency_key, intended_head_oid,
            expected_remote_absent, last_error, created_at, updated_at, completed_at
         ) VALUES (
            'effect-1', 'repair-1', 'push_branch', 'failed', 'push:repair-1', 'abc123',
            1, 'push never reached remote',
            '2026-08-06T00:00:00+00:00', '2026-08-06T00:01:00+00:00', '2026-08-06T00:01:00+00:00'
         )",
        [],
    )
    .expect("closed failed effect is allowed");

    // Closing the failed row frees the attempt's single open-effect slot.
    conn.execute(
        "INSERT INTO agent_workspace_repair_effects (
            id, attempt_id, kind, status, idempotency_key, created_at, updated_at
         ) VALUES (
            'effect-2', 'repair-1', 'push_branch', 'pending', 'push:repair-1#2',
            '2026-08-06T00:02:00+00:00', '2026-08-06T00:02:00+00:00'
         )",
        [],
    )
    .expect("a new open effect is allowed once the failed row is closed");
}

#[test]
fn fresh_schema_rejects_failed_effect_with_receipt() {
    let conn = open_memory_connection().expect("open memory database");
    run_migrations(&conn).expect("run fresh migrations");
    seed_attempt(&conn, "conversation-1", "repair-1");

    assert!(conn
        .execute(
            "INSERT INTO agent_workspace_repair_effects (
                id, attempt_id, kind, status, idempotency_key, receipt_json,
                created_at, updated_at, completed_at
             ) VALUES (
                'effect-invalid', 'repair-1', 'push_branch', 'failed', 'push:repair-1',
                '{\"remote_oid\":\"abc\"}',
                '2026-08-06T00:00:00+00:00', '2026-08-06T00:01:00+00:00', '2026-08-06T00:01:00+00:00'
             )",
            [],
        )
        .is_err());
}

#[test]
fn fresh_schema_rejects_in_flight_effect_with_completed_at() {
    let conn = open_memory_connection().expect("open memory database");
    run_migrations(&conn).expect("run fresh migrations");
    seed_attempt(&conn, "conversation-1", "repair-1");

    assert!(conn
        .execute(
            "INSERT INTO agent_workspace_repair_effects (
                id, attempt_id, kind, status, idempotency_key,
                created_at, updated_at, completed_at
             ) VALUES (
                'effect-invalid', 'repair-1', 'push_branch', 'in_flight', 'push:repair-1',
                '2026-08-06T00:00:00+00:00', '2026-08-06T00:01:00+00:00', '2026-08-06T00:01:00+00:00'
             )",
            [],
        )
        .is_err());
}

#[test]
fn upgrade_backfills_completed_at_for_pre_existing_failed_rows() {
    let conn = open_memory_connection().expect("open memory database");
    run_migrations_through(&conn, PREVIOUS_SCHEMA_VERSION).expect("run prior migrations");
    seed_attempt(&conn, "conversation-1", "repair-1");
    conn.execute(
        "INSERT INTO agent_workspace_repair_effects (
            id, attempt_id, kind, status, idempotency_key, intended_head_oid,
            expected_remote_absent, last_error, created_at, updated_at
         ) VALUES (
            'effect-1', 'repair-1', 'push_branch', 'failed', 'push:repair-1', 'abc123',
            1, 'push never reached remote',
            '2026-08-06T00:00:00+00:00', '2026-08-06T00:01:00+00:00'
         )",
        [],
    )
    .expect("seed pre-migration open failed row");

    run_migrations(&conn).expect("upgrade through the failed-completed-at migration");

    let completed_at: Option<String> = conn
        .query_row(
            "SELECT completed_at FROM agent_workspace_repair_effects WHERE id = 'effect-1'",
            [],
            |row| row.get(0),
        )
        .expect("load backfilled row");
    assert_eq!(completed_at.as_deref(), Some("2026-08-06T00:01:00+00:00"));
    assert_eq!(get_schema_version(&conn).unwrap(), SCHEMA_VERSION);
}
