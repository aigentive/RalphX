//! Tests for migration v20260731023949: agent run identity

use rusqlite::Connection;

use super::v20260731023949_agent_run_identity;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    conn.execute_batch("CREATE TABLE agent_runs (id TEXT PRIMARY KEY); INSERT INTO agent_runs (id) VALUES ('legacy-run');")
        .unwrap();
    v20260731023949_agent_run_identity::migrate(&conn).unwrap();

    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(agent_runs)")
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(columns.contains(&"agent_name".to_string()));
    assert!(columns.contains(&"launch_role".to_string()));
    assert!(columns.contains(&"runtime_source".to_string()));

    let legacy: (Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT agent_name, launch_role, runtime_source FROM agent_runs WHERE id = 'legacy-run'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(legacy, (None, None, None));
}
