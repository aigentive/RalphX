use crate::infrastructure::sqlite::migrations::v60_metrics_working_days::migrate;
use rusqlite::Connection;

#[test]
fn adds_working_days_per_week_column() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE project_metrics_config (
            project_id TEXT PRIMARY KEY,
            simple_base_hours REAL NOT NULL DEFAULT 2.0,
            medium_base_hours REAL NOT NULL DEFAULT 4.0,
            complex_base_hours REAL NOT NULL DEFAULT 8.0,
            calendar_factor REAL NOT NULL DEFAULT 1.5,
            updated_at TEXT
        );",
    )
    .unwrap();

    migrate(&conn).unwrap();

    // Column exists with default 5
    conn.execute(
        "INSERT INTO project_metrics_config (project_id) VALUES ('p1')",
        [],
    )
    .unwrap();
    let val: i64 = conn
        .query_row(
            "SELECT working_days_per_week FROM project_metrics_config WHERE project_id = 'p1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(val, 5);
}

#[test]
fn idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE project_metrics_config (
            project_id TEXT PRIMARY KEY,
            simple_base_hours REAL NOT NULL DEFAULT 2.0,
            medium_base_hours REAL NOT NULL DEFAULT 4.0,
            complex_base_hours REAL NOT NULL DEFAULT 8.0,
            calendar_factor REAL NOT NULL DEFAULT 1.5,
            updated_at TEXT
        );",
    )
    .unwrap();

    migrate(&conn).unwrap();
    migrate(&conn).unwrap(); // Should not fail
}
