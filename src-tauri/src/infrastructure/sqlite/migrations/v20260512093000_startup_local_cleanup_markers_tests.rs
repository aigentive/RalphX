//! Tests for migration v20260512093000: startup local cleanup markers

use rusqlite::Connection;

use super::helpers;
use super::v20260512093000_startup_local_cleanup_markers;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("create in-memory db");
    conn.execute_batch(
        "CREATE TABLE plan_branches (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            status TEXT NOT NULL,
            pr_status TEXT NULL
        );
        CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            publication_pr_status TEXT NULL
        );",
    )
    .expect("create test schema");
    conn
}

#[test]
fn startup_local_cleanup_marker_columns_are_added() {
    let conn = setup_test_db();

    v20260512093000_startup_local_cleanup_markers::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "plan_branches",
        "local_cleanup_status"
    ));
    assert!(helpers::column_exists(
        &conn,
        "plan_branches",
        "local_cleanup_checked_at"
    ));
    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspaces",
        "local_cleanup_status"
    ));
    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspaces",
        "local_cleanup_checked_at"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_plan_branches_project_cleanup"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspaces_project_cleanup"
    ));
}

#[test]
fn startup_local_cleanup_marker_migration_is_idempotent() {
    let conn = setup_test_db();

    v20260512093000_startup_local_cleanup_markers::migrate(&conn).unwrap();
    v20260512093000_startup_local_cleanup_markers::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "plan_branches",
        "local_cleanup_status"
    ));
    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspaces",
        "local_cleanup_status"
    ));
}
