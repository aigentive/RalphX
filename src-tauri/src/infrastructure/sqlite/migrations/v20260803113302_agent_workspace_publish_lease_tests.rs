//! Tests for migration v20260803113302: agent workspace publish lease

use rusqlite::Connection;

use super::v20260803113302_agent_workspace_publish_lease::migrate;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (conversation_id TEXT PRIMARY KEY);",
    )
    .expect("seed workspace table");
    conn
}

#[test]
fn migration_adds_idempotent_nullable_publish_lease_columns() {
    let conn = setup_test_db();
    migrate(&conn).expect("first migration should succeed");
    migrate(&conn).expect("second migration should succeed");
    for column in [
        "publish_lease_owner_run_id",
        "publish_lease_token",
        "publish_lease_heartbeat_at",
    ] {
        let found: bool = conn
            .prepare("PRAGMA table_info(agent_conversation_workspaces)")
            .expect("prepare table info")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("read table info")
            .filter_map(Result::ok)
            .any(|name| name == column);
        assert!(found, "migration must add {column}");
    }
}
