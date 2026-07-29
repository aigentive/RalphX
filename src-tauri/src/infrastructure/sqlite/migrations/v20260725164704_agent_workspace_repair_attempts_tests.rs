//! Tests for migration v20260725164704: agent workspace repair attempts

use rusqlite::params;

use super::{get_schema_version, run_migrations, run_migrations_through, SCHEMA_VERSION};
use crate::infrastructure::sqlite::connection::open_memory_connection;

const PREVIOUS_SCHEMA_VERSION: i64 = 20260723100604;

fn seed_workspace(conn: &rusqlite::Connection, conversation_id: &str) {
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
            '2026-07-25T00:00:00+00:00', '2026-07-25T00:00:00+00:00'
         )",
        [conversation_id],
    )
    .expect("seed workspace");
}

#[test]
fn fresh_schema_enforces_repair_attempt_and_effect_constraints() {
    let conn = open_memory_connection().expect("open memory database");
    run_migrations(&conn).expect("run fresh migrations");
    seed_workspace(&conn, "conversation-1");

    assert_eq!(get_schema_version(&conn).unwrap(), SCHEMA_VERSION);

    conn.execute(
        "INSERT INTO git_target_leases (
            git_common_dir, target_ref, identity_version, owner_kind, owner_id,
            fencing_epoch, acquired_at, recovery_state
         ) VALUES (
            '/repo/.git', 'refs/heads/ralphx/project/agent-conversation-1', 1,
            'agent_workspace_repair', 'repair-1', 1,
            '2026-07-25T00:00:00+00:00', 'ready'
         )",
        [],
    )
    .expect("new repair lease owner is allowed");

    let insert_attempt = |id: &str, generation: i64, settled_at: Option<&str>| {
        conn.execute(
            "INSERT INTO agent_workspace_repair_attempts (
                id, conversation_id, generation, source, phase, continuation,
                target_base_ref, review_required, auto_publish_enabled, auto_merge_desired,
                created_at, updated_at, outcome, settled_at
             ) VALUES (
                ?1, 'conversation-1', ?2, 'base_update', 'requested', 'update_only',
                'origin/main', 0, 0, 0,
                '2026-07-25T00:00:00+00:00', '2026-07-25T00:00:00+00:00',
                CASE WHEN ?3 IS NULL THEN NULL ELSE 'succeeded' END, ?3
             )",
            params![id, generation, settled_at],
        )
    };

    insert_attempt("repair-1", 1, None).expect("insert active repair attempt");
    assert!(insert_attempt("repair-2", 2, None).is_err());
    assert!(insert_attempt("repair-invalid-generation", 0, None).is_err());
    assert!(conn
        .execute(
            "INSERT INTO agent_workspace_repair_attempts (
                id, conversation_id, generation, source, phase, continuation,
                target_base_ref, review_required, auto_publish_enabled, auto_merge_desired,
                pending_reasons_json, created_at, updated_at
             ) VALUES (
                'repair-invalid-reasons', 'conversation-1', 2, 'base_update', 'requested',
                'update_only', 'origin/main', 0, 0, 0, '{}',
                '2026-07-25T00:00:00+00:00', '2026-07-25T00:00:00+00:00'
             )",
            [],
        )
        .is_err());

    conn.execute(
        "UPDATE agent_workspace_repair_attempts
         SET outcome = 'succeeded', settled_at = '2026-07-25T00:01:00+00:00'
         WHERE id = 'repair-1'",
        [],
    )
    .expect("settle first attempt");
    insert_attempt("repair-2", 2, None).expect("insert successor attempt");

    conn.execute(
        "INSERT INTO agent_workspace_repair_effects (
            id, attempt_id, kind, status, idempotency_key, created_at, updated_at
         ) VALUES (
            'effect-1', 'repair-2', 'push_branch', 'pending', 'push:repair-2',
            '2026-07-25T00:00:00+00:00', '2026-07-25T00:00:00+00:00'
         )",
        [],
    )
    .expect("insert open effect");
    assert!(conn
        .execute(
            "INSERT INTO agent_workspace_repair_effects (
                id, attempt_id, kind, status, idempotency_key, created_at, updated_at
             ) VALUES (
                'effect-2', 'repair-2', 'create_pr', 'pending', 'create:repair-2',
                '2026-07-25T00:00:00+00:00', '2026-07-25T00:00:00+00:00'
             )",
            [],
        )
        .is_err());
    assert!(conn
        .execute(
            "INSERT INTO agent_workspace_repair_effects (
                id, attempt_id, kind, status, idempotency_key, expected_remote_absent,
                expected_remote_oid, created_at, updated_at
             ) VALUES (
                'effect-invalid-expectation', 'repair-2', 'push_branch', 'observed',
                'invalid:repair-2', 1, 'abc',
                '2026-07-25T00:00:00+00:00', '2026-07-25T00:00:00+00:00'
             )",
            [],
        )
        .is_err());
}

#[test]
fn upgrade_from_app_state_update_channel_preserves_active_lease_rows() {
    let conn = open_memory_connection().expect("open memory database");
    run_migrations_through(&conn, PREVIOUS_SCHEMA_VERSION).expect("run prior migrations");
    conn.execute(
        "INSERT INTO git_target_leases (
            git_common_dir, target_ref, identity_version, owner_kind, owner_id,
            fencing_epoch, acquired_at, recovery_state, mutation_claim_id, mutation_kind,
            mutation_started_at
         ) VALUES (
            '/repo/.git', 'refs/heads/main', 1, 'manual', 'manual-owner', 4,
            '2026-07-23T00:00:00+00:00', 'mutation_in_flight', 'claim-1', 'fetch',
            '2026-07-23T00:00:01+00:00'
         )",
        [],
    )
    .expect("seed legacy lease");

    run_migrations(&conn).expect("upgrade through repair migration");

    let row: (String, String, i64, String, String) = conn
        .query_row(
            "SELECT owner_kind, owner_id, fencing_epoch, mutation_claim_id, mutation_kind
             FROM git_target_leases
             WHERE git_common_dir = '/repo/.git' AND target_ref = 'refs/heads/main'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("load preserved lease");
    assert_eq!(
        row,
        (
            "manual".to_string(),
            "manual-owner".to_string(),
            4,
            "claim-1".to_string(),
            "fetch".to_string(),
        )
    );
    assert_eq!(get_schema_version(&conn).unwrap(), SCHEMA_VERSION);
}
