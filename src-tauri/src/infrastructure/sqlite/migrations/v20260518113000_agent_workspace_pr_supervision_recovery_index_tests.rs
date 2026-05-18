//! Tests for migration v20260518113000: agent workspace PR supervision recovery index

use rusqlite::Connection;

use super::helpers;
use super::v20260518113000_agent_workspace_pr_supervision_recovery_index;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("create in-memory db");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            status TEXT NOT NULL,
            mode TEXT NOT NULL,
            publication_push_status TEXT NULL,
            pr_supervision_status TEXT NULL,
            updated_at TEXT NOT NULL
        );",
    )
    .expect("create test schema");
    conn
}

#[test]
fn agent_workspace_pr_supervision_recovery_index_is_added() {
    let conn = setup_test_db();

    v20260518113000_agent_workspace_pr_supervision_recovery_index::migrate(&conn).unwrap();

    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspaces_pr_supervision_recovery"
    ));
}

#[test]
fn agent_workspace_pr_supervision_recovery_index_migration_is_idempotent() {
    let conn = setup_test_db();

    v20260518113000_agent_workspace_pr_supervision_recovery_index::migrate(&conn).unwrap();
    v20260518113000_agent_workspace_pr_supervision_recovery_index::migrate(&conn).unwrap();

    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspaces_pr_supervision_recovery"
    ));
}
