//! Tests for migration v71: target_project column on task_proposals

use rusqlite::Connection;

use super::helpers;
use super::v71_add_target_project_to_proposals;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    conn.execute_batch(
        "CREATE TABLE task_proposals (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL
        );",
    )
    .expect("Failed to create test schema");

    conn
}

#[test]
fn test_target_project_column_added() {
    let conn = setup_test_db();

    assert!(
        !helpers::column_exists(&conn, "task_proposals", "target_project"),
        "target_project should not exist before migration"
    );

    v71_add_target_project_to_proposals::migrate(&conn).unwrap();

    assert!(
        helpers::column_exists(&conn, "task_proposals", "target_project"),
        "target_project should exist after migration"
    );
}

#[test]
fn test_existing_proposals_have_null_target_project() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO task_proposals (id, title) VALUES ('p1', 'Pre-migration Proposal')",
        [],
    )
    .unwrap();

    v71_add_target_project_to_proposals::migrate(&conn).unwrap();

    let val: Option<String> = conn
        .query_row(
            "SELECT target_project FROM task_proposals WHERE id = 'p1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert!(
        val.is_none(),
        "existing proposals should have NULL target_project"
    );
}

#[test]
fn test_new_proposal_can_set_target_project() {
    let conn = setup_test_db();
    v71_add_target_project_to_proposals::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO task_proposals (id, title, target_project) VALUES ('p2', 'New Proposal', 'project-abc')",
        [],
    )
    .unwrap();

    let val: Option<String> = conn
        .query_row(
            "SELECT target_project FROM task_proposals WHERE id = 'p2'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(val.as_deref(), Some("project-abc"));
}

#[test]
fn test_migration_idempotent() {
    let conn = setup_test_db();

    v71_add_target_project_to_proposals::migrate(&conn).unwrap();
    v71_add_target_project_to_proposals::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "task_proposals",
        "target_project"
    ));
}
