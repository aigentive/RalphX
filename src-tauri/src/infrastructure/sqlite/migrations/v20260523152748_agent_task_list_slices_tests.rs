use rusqlite::Connection;

use super::v20260523152748_agent_task_list_slices;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    create_legacy_agent_task_tables(&conn);

    v20260523152748_agent_task_list_slices::migrate(&conn).unwrap();

    let columns = table_columns(&conn, "agent_task_lists");
    assert!(columns.contains(&"list_sequence".to_string()));

    conn.execute(
        "INSERT INTO agent_task_lists (
            id, project_id, scope_type, scope_id, list_sequence, name,
            created_by_agent, next_task_number, created_at, updated_at
        ) VALUES (
            'list-2', NULL, 'conversation', 'conv-1', 2, NULL,
            'worker', 1, '2026-05-23T00:00:00Z', '2026-05-23T00:00:00Z'
        )",
        [],
    )
    .expect("same scope with a new sequence should be allowed");

    let duplicate_sequence = conn.execute(
        "INSERT INTO agent_task_lists (
            id, project_id, scope_type, scope_id, list_sequence, name,
            created_by_agent, next_task_number, created_at, updated_at
        ) VALUES (
            'list-duplicate', NULL, 'conversation', 'conv-1', 2, NULL,
            'worker', 1, '2026-05-23T00:00:00Z', '2026-05-23T00:00:00Z'
        )",
        [],
    );
    assert!(duplicate_sequence.is_err());
}

fn create_legacy_agent_task_tables(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE projects (
            id TEXT PRIMARY KEY
        );

        CREATE TABLE agent_task_lists (
            id TEXT PRIMARY KEY,
            project_id TEXT,
            scope_type TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            name TEXT,
            created_by_agent TEXT,
            next_task_number INTEGER NOT NULL DEFAULT 1,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(scope_type, scope_id)
        );

        CREATE TABLE agent_tasks (
            id TEXT NOT NULL,
            task_list_id TEXT NOT NULL REFERENCES agent_task_lists(id) ON DELETE CASCADE,
            task_number INTEGER NOT NULL,
            title TEXT NOT NULL,
            details TEXT NOT NULL,
            active_label TEXT,
            owner_agent TEXT,
            state TEXT NOT NULL CHECK (state IN ('open', 'active', 'done', 'dropped')),
            metadata_json TEXT,
            version INTEGER NOT NULL DEFAULT 1,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            completed_at DATETIME,
            PRIMARY KEY (task_list_id, id),
            UNIQUE(task_list_id, task_number)
        );

        INSERT INTO agent_task_lists (
            id, project_id, scope_type, scope_id, name, created_by_agent,
            next_task_number, created_at, updated_at
        ) VALUES (
            'list-1', NULL, 'conversation', 'conv-1', NULL, 'worker',
            2, '2026-05-23T00:00:00Z', '2026-05-23T00:00:00Z'
        );",
    )
    .unwrap();
}

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("table info should prepare");
    stmt.query_map([], |row| row.get::<_, String>(1))
        .expect("table info should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("table columns should parse")
}
