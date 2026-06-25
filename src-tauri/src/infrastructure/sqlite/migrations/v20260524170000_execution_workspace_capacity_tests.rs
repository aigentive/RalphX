use rusqlite::Connection;

use super::{helpers, v20260524170000_execution_workspace_capacity};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_adds_workspace_capacity_default() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE global_execution_settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            global_max_concurrent INTEGER NOT NULL DEFAULT 20,
            global_ideation_max INTEGER NOT NULL DEFAULT 4,
            allow_ideation_borrow_idle_execution INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL
        );
        INSERT INTO global_execution_settings (
            id,
            global_max_concurrent,
            global_ideation_max,
            allow_ideation_borrow_idle_execution,
            updated_at
        ) VALUES (
            1,
            20,
            4,
            0,
            '2026-05-24T00:00:00+00:00'
        );",
    )
    .unwrap();

    v20260524170000_execution_workspace_capacity::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "global_execution_settings",
        "workspace_max_concurrent"
    ));

    let values: (i64, i64) = conn
        .query_row(
            "SELECT workspace_max_concurrent, global_ideation_max
             FROM global_execution_settings WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(values.0, 10);
    assert_eq!(values.1, 4);
}
