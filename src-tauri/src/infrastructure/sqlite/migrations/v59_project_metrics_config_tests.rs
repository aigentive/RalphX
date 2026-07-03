#[cfg(test)]
mod tests {
    use crate::infrastructure::sqlite::migrations::v59_project_metrics_config::migrate;
    use rusqlite::Connection;

    fn setup(conn: &Connection) {
        conn.execute_batch(
            "
            CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL);
        ",
        )
        .unwrap();
    }

    #[test]
    fn test_migration_creates_table() {
        let conn = Connection::open_in_memory().unwrap();
        setup(&conn);
        migrate(&conn).expect("migration should succeed");

        // Verify table exists
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='project_metrics_config'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_migration_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        setup(&conn);
        migrate(&conn).expect("first migration");
        migrate(&conn).expect("second migration should be idempotent (IF NOT EXISTS)");
    }

    #[test]
    fn test_table_columns_exist() {
        let conn = Connection::open_in_memory().unwrap();
        setup(&conn);
        migrate(&conn).unwrap();

        // Insert a project first to satisfy FK constraint, then insert config
        conn.execute(
            "INSERT INTO projects (id, name) VALUES ('proj1', 'Test Project')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_metrics_config (project_id, simple_base_hours, medium_base_hours, complex_base_hours, calendar_factor) VALUES ('proj1', 3.0, 6.0, 12.0, 2.0)",
            [],
        ).unwrap();

        let (s, m, c, f): (f64, f64, f64, f64) = conn.query_row(
            "SELECT simple_base_hours, medium_base_hours, complex_base_hours, calendar_factor FROM project_metrics_config WHERE project_id = 'proj1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        ).unwrap();

        assert_eq!(s, 3.0);
        assert_eq!(m, 6.0);
        assert_eq!(c, 12.0);
        assert_eq!(f, 2.0);
    }
}
