#[cfg(test)]
mod tests {
    use crate::infrastructure::sqlite::connection::open_memory_connection;
    use crate::infrastructure::sqlite::migrations::run_migrations;

    #[test]
    fn test_migration_creates_table() {
        let conn = open_memory_connection().expect("open memory db");
        run_migrations(&conn).expect("run migrations");

        // Verify table exists
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='ideation_effort_settings'",
                [],
                |row| row.get(0),
            )
            .expect("query table existence");
        assert_eq!(count, 1, "ideation_effort_settings table should exist");

        // Verify unique index exists
        let idx_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_ideation_effort_project'",
                [],
                |row| row.get(0),
            )
            .expect("query index existence");
        assert_eq!(
            idx_count, 1,
            "idx_ideation_effort_project index should exist"
        );

        // Verify we can insert a global row (project_id IS NULL)
        conn.execute(
            "INSERT INTO ideation_effort_settings (project_id, primary_effort, verifier_effort) VALUES (NULL, 'inherit', 'inherit')",
            [],
        ).expect("insert global row");

        // Verify we can insert a per-project row
        conn.execute(
            "INSERT INTO ideation_effort_settings (project_id, primary_effort, verifier_effort) VALUES ('proj-test', 'high', 'medium')",
            [],
        ).expect("insert project row");

        // Verify unique constraint prevents duplicate project_id
        let result = conn.execute(
            "INSERT INTO ideation_effort_settings (project_id, primary_effort, verifier_effort) VALUES ('proj-test', 'low', 'low')",
            [],
        );
        assert!(result.is_err(), "should reject duplicate project_id");
    }
}
