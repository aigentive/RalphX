use rusqlite::Connection;

use super::v20260519180000_agent_tasks;

#[test]
fn test_agent_tasks_migration_creates_tables_and_columns() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE projects (id TEXT PRIMARY KEY);")
        .unwrap();

    v20260519180000_agent_tasks::migrate(&conn).unwrap();

    let tables: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name LIKE 'agent_task%'
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(
        tables,
        vec![
            "agent_task_dependencies",
            "agent_task_events",
            "agent_task_lists",
            "agent_tasks",
        ]
    );

    let task_columns: Vec<String> = conn
        .prepare("PRAGMA table_info(agent_tasks)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    for expected in [
        "id",
        "task_list_id",
        "task_number",
        "title",
        "details",
        "owner_agent",
        "state",
        "metadata_json",
        "version",
    ] {
        assert!(task_columns.iter().any(|column| column == expected));
    }
}
