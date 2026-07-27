//! Tests for migration v20260724222347: agent task assignment planned run identity

use rusqlite::Connection;

use super::helpers;
use super::v20260724222347_agent_task_assignment_planned_run_identity;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_task_delegate_assignments (
            id TEXT PRIMARY KEY,
            state TEXT NOT NULL,
            delegated_agent_run_id TEXT
        );
        INSERT INTO agent_task_delegate_assignments (
            id, state, delegated_agent_run_id
        ) VALUES (
            'existing-assignment', 'reserved', 'future-run'
        ), (
            'active-assignment', 'active', 'bound-run'
        );",
    )
    .unwrap();
    conn
}

#[test]
fn migration_adds_unique_planned_run_identity_without_requiring_a_run_row() {
    let conn = setup_test_db();
    v20260724222347_agent_task_assignment_planned_run_identity::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "agent_task_delegate_assignments",
        "planned_delegated_agent_run_id"
    ));
    let migrated: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT planned_delegated_agent_run_id, delegated_agent_run_id
             FROM agent_task_delegate_assignments
             WHERE id = 'existing-assignment'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(migrated, (Some("future-run".to_string()), None));
    let active: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT planned_delegated_agent_run_id, delegated_agent_run_id
             FROM agent_task_delegate_assignments
             WHERE id = 'active-assignment'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        active,
        (Some("bound-run".to_string()), Some("bound-run".to_string()))
    );
    let duplicate = conn.execute(
        "INSERT INTO agent_task_delegate_assignments (
            id, state, planned_delegated_agent_run_id
         ) VALUES ('other-assignment', 'reserved', 'future-run')",
        [],
    );
    assert!(duplicate.is_err());

    v20260724222347_agent_task_assignment_planned_run_identity::migrate(&conn).unwrap();
}
