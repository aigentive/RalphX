//! Tests for migration v20260724113627: agent task delegate assignments

use rusqlite::Connection;

use super::v20260724113627_agent_task_delegate_assignments;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE delegated_sessions (id TEXT PRIMARY KEY);
         CREATE TABLE agent_runs (id TEXT PRIMARY KEY);
         CREATE TABLE agent_task_lists (id TEXT PRIMARY KEY);
         CREATE TABLE agent_tasks (
            id TEXT NOT NULL,
            task_list_id TEXT NOT NULL,
            PRIMARY KEY (task_list_id, id),
            FOREIGN KEY (task_list_id) REFERENCES agent_task_lists(id)
         );",
    )
    .unwrap();
    conn
}

#[test]
fn migration_creates_attempt_scoped_assignment_constraints() {
    let conn = setup_test_db();
    v20260724113627_agent_task_delegate_assignments::migrate(&conn).unwrap();

    conn.execute_batch(
        "INSERT INTO delegated_sessions (id) VALUES ('session-1'), ('session-2');
         INSERT INTO agent_runs (id) VALUES ('caller-1'), ('caller-2'), ('delegated-1');
         INSERT INTO agent_task_lists (id) VALUES ('list-1');
         INSERT INTO agent_tasks (id, task_list_id)
            VALUES ('task-1', 'list-1'), ('task-2', 'list-1');
         INSERT INTO agent_task_delegate_assignments (
            id, delegated_session_id, attempt_number, caller_agent_run_id,
            delegated_agent_run_id, task_list_id, task_id, delegate_agent_name, state
         ) VALUES (
            'assignment-1', 'session-1', 1, 'caller-1',
            'delegated-1', 'list-1', 'task-1', 'worker', 'reserved'
         );",
    )
    .unwrap();

    let same_session = conn.execute(
        "INSERT INTO agent_task_delegate_assignments (
            id, delegated_session_id, attempt_number, caller_agent_run_id,
            task_list_id, task_id, delegate_agent_name, state
         ) VALUES (
            'assignment-2', 'session-1', 2, 'caller-2',
            'list-1', 'task-2', 'worker', 'active'
         )",
        [],
    );
    assert!(same_session.is_err());

    let same_task = conn.execute(
        "INSERT INTO agent_task_delegate_assignments (
            id, delegated_session_id, attempt_number, caller_agent_run_id,
            task_list_id, task_id, delegate_agent_name, state
         ) VALUES (
            'assignment-3', 'session-2', 1, 'caller-2',
            'list-1', 'task-1', 'worker', 'active'
         )",
        [],
    );
    assert!(same_task.is_err());

    conn.execute(
        "UPDATE agent_task_delegate_assignments
         SET state = 'failed'
         WHERE id = 'assignment-1'",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_task_delegate_assignments (
            id, delegated_session_id, attempt_number, caller_agent_run_id,
            task_list_id, task_id, delegate_agent_name, state
         ) VALUES (
            'assignment-4', 'session-1', 2, 'caller-2',
            'list-1', 'task-1', 'worker', 'reserved'
         )",
        [],
    )
    .unwrap();

    let reused_run = conn.execute(
        "INSERT INTO agent_task_delegate_assignments (
            id, delegated_session_id, attempt_number, caller_agent_run_id,
            delegated_agent_run_id, task_list_id, task_id, delegate_agent_name, state
         ) VALUES (
            'assignment-5', 'session-2', 1, 'caller-2',
            'delegated-1', 'list-1', 'task-2', 'worker', 'failed'
         )",
        [],
    );
    assert!(reused_run.is_err());
}
