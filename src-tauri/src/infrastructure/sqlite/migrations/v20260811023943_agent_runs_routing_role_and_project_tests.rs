//! Tests for migration v20260811023943: agent runs routing role and project

use rusqlite::Connection;

use super::v20260811023943_agent_runs_routing_role_and_project;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_runs (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL
        );",
    )
    .expect("Failed to create agent_runs table");
    conn
}

fn columns(conn: &Connection) -> Vec<String> {
    let mut statement = conn
        .prepare("PRAGMA table_info(agent_runs)")
        .expect("pragma should prepare");
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("pragma should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("pragma rows should read");
    rows
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    v20260811023943_agent_runs_routing_role_and_project::migrate(&conn).unwrap();

    let columns = columns(&conn);
    assert!(columns.contains(&"routing_role".to_string()));
    assert!(columns.contains(&"project_id".to_string()));
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260811023943_agent_runs_routing_role_and_project::migrate(&conn).unwrap();
    v20260811023943_agent_runs_routing_role_and_project::migrate(&conn).unwrap();

    let columns = columns(&conn);
    assert_eq!(
        columns
            .iter()
            .filter(|name| *name == "routing_role")
            .count(),
        1
    );
    assert_eq!(
        columns.iter().filter(|name| *name == "project_id").count(),
        1
    );
}

#[test]
fn existing_rows_read_the_new_columns_as_null() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO agent_runs (id, conversation_id, status, started_at)
         VALUES ('run-1', 'conv-1', 'running', '2026-08-11T00:00:00+00:00')",
        [],
    )
    .expect("legacy row should insert");

    v20260811023943_agent_runs_routing_role_and_project::migrate(&conn).unwrap();

    let (role, project): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT routing_role, project_id FROM agent_runs WHERE id = 'run-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("legacy row should read");
    assert_eq!(
        role, None,
        "pre-existing runs must deny, not inherit a role"
    );
    assert_eq!(project, None);
}
