//! Tests for migration v20260802194326: agent workspace repair explicit publish consent

use rusqlite::Connection;

use super::v20260802194326_agent_workspace_repair_explicit_publish_consent::migrate;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_workspace_repair_attempts (
            id TEXT PRIMARY KEY,
            explicit_publish_requested INTEGER NOT NULL DEFAULT 0
        );",
    )
    .expect("seed repair attempts table");
    conn
}

fn setup_pre_migration_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch("CREATE TABLE agent_workspace_repair_attempts (id TEXT PRIMARY KEY);")
        .expect("seed pre-migration repair attempts table");
    conn
}

#[test]
fn migration_adds_explicit_publish_consent_with_safe_default() {
    let conn = setup_pre_migration_db();
    conn.execute(
        "INSERT INTO agent_workspace_repair_attempts (id) VALUES ('existing')",
        [],
    )
    .expect("seed existing repair attempt");

    migrate(&conn).expect("migration should add explicit publish consent");

    let consent: bool = conn
        .query_row(
            "SELECT explicit_publish_requested FROM agent_workspace_repair_attempts WHERE id = 'existing'",
            [],
            |row| row.get(0),
        )
        .expect("read consent default");
    assert!(
        !consent,
        "existing attempts must not gain publish authority"
    );
}

#[test]
fn explicit_publish_consent_migration_is_idempotent_and_preserves_existing_consent() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO agent_workspace_repair_attempts (id, explicit_publish_requested) VALUES ('consented', 1)",
        [],
    )
    .expect("seed consented repair attempt");

    migrate(&conn).expect("first migration run should succeed");
    migrate(&conn).expect("second migration run should succeed");

    let consent: bool = conn
        .query_row(
            "SELECT explicit_publish_requested FROM agent_workspace_repair_attempts WHERE id = 'consented'",
            [],
            |row| row.get(0),
        )
        .expect("read preserved consent");
    assert!(consent);
}
