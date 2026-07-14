//! Tests for migration v20260712190416: branch update authority

use rusqlite::{params, Connection};

use super::v20260712190416_branch_update_authority;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE tasks (id TEXT PRIMARY KEY, internal_status TEXT NOT NULL);
         CREATE TABLE task_state_history (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             to_status TEXT NOT NULL
         );",
    )
    .unwrap();
    conn
}

#[test]
fn migration_creates_operation_lease_and_mutation_authority_tables() {
    let conn = setup_test_db();
    v20260712190416_branch_update_authority::migrate(&conn).unwrap();

    for table in ["branch_update_operations", "git_target_leases"] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing table {table}");
    }
}

fn seed_task_and_history(conn: &Connection, task_id: &str, history_id: &str) {
    conn.execute(
        "INSERT INTO tasks (id, internal_status) VALUES (?1, 'executing')",
        [task_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status) VALUES (?1, ?2, 'executing')",
        params![history_id, task_id],
    )
    .unwrap();
}

#[test]
fn migration_enforces_one_active_operation_per_task_but_keeps_history() {
    let conn = setup_test_db();
    v20260712190416_branch_update_authority::migrate(&conn).unwrap();
    seed_task_and_history(&conn, "task-1", "history-1");

    let insert = |id: &str, settled_at: Option<&str>| {
        conn.execute(
            "INSERT INTO branch_update_operations (
                id, task_id, direction, phase, continuation, originating_history_id,
                source_branch, target_branch, workspace_ownership, capacity_ownership,
                git_common_dir, target_ref, target_identity_version, target_lease_epoch,
                settled_at
             ) VALUES (?1, 'task-1', 'plan_branch', 'programmatic', 'resume_execution',
                'history-1', 'main', 'plan', 'operation_worktree', 'inherited',
                '/repo/.git', 'refs/heads/plan', 1, 1, ?2)",
            params![id, settled_at],
        )
    };

    insert("operation-1", None).unwrap();
    assert!(insert("operation-2", None).is_err());
    conn.execute(
        "UPDATE branch_update_operations SET phase = 'settled', settled_at = '2026-07-12T00:00:00+00:00' WHERE id = 'operation-1'",
        [],
    )
    .unwrap();
    insert("operation-2", None).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM branch_update_operations WHERE task_id = 'task-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn migration_enforces_one_active_owner_per_canonical_target() {
    let conn = setup_test_db();
    v20260712190416_branch_update_authority::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO git_target_leases (
            git_common_dir, target_ref, identity_version, owner_kind, owner_id,
            fencing_epoch, acquired_at, recovery_state
         ) VALUES ('/repo/.git', 'refs/heads/main', 1, 'manual', 'owner-1', 1,
            '2026-07-12T00:00:00+00:00', 'ready')",
        [],
    )
    .unwrap();

    let duplicate = conn.execute(
        "INSERT INTO git_target_leases (
            git_common_dir, target_ref, identity_version, owner_kind, owner_id,
            fencing_epoch, acquired_at, recovery_state
         ) VALUES ('/repo/.git', 'refs/heads/main', 1, 'manual', 'owner-2', 1,
            '2026-07-12T00:00:00+00:00', 'ready')",
        [],
    );
    assert!(duplicate.is_err());
}

#[test]
fn mutation_claim_columns_make_in_flight_authority_durable() {
    let conn = setup_test_db();
    v20260712190416_branch_update_authority::migrate(&conn).unwrap();
    seed_task_and_history(&conn, "task-1", "history-1");
    conn.execute(
        "INSERT INTO git_target_leases (
            git_common_dir, target_ref, identity_version, owner_kind, owner_task_id,
            owner_id, fencing_epoch, acquired_at, recovery_state, mutation_claim_id,
            mutation_kind, mutation_process_group_id, mutation_started_at
         ) VALUES ('/repo/.git', 'refs/heads/main', 1, 'branch_update_operation',
            'task-1', 'operation-1', 9, '2026-07-12T00:00:00+00:00',
            'mutation_in_flight', 'claim-1', 'merge', 4242,
            '2026-07-12T00:00:01+00:00')",
        [],
    )
    .unwrap();

    let row: (String, String, i64) = conn
        .query_row(
            "SELECT mutation_claim_id, mutation_kind, mutation_process_group_id
             FROM git_target_leases WHERE git_common_dir = '/repo/.git' AND target_ref = 'refs/heads/main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(row, ("claim-1".into(), "merge".into(), 4242));
}
