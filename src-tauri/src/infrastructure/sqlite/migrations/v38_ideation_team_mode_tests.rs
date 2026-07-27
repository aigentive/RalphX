// Tests for v38 ideation_team_mode migration

#[cfg(test)]
mod tests {
    use crate::infrastructure::sqlite::migrations::helpers::column_exists;
    use crate::infrastructure::sqlite::migrations::run_migrations_through;
    use crate::infrastructure::sqlite::migrations::v38_ideation_team_mode;
    use crate::infrastructure::sqlite::open_connection;
    use std::path::PathBuf;

    fn setup_db() -> rusqlite::Connection {
        let conn = open_connection(&PathBuf::from(":memory:")).unwrap();
        run_migrations_through(&conn, 38).unwrap();
        // Insert a project to satisfy FK constraints on ideation_sessions
        conn.execute(
            "INSERT INTO projects (id, name, working_directory) VALUES ('p-1', 'Test', '/tmp/test')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_team_mode_column_exists() {
        let conn = setup_db();
        assert!(column_exists(&conn, "ideation_sessions", "team_mode"));
    }

    #[test]
    fn test_team_config_json_column_exists() {
        let conn = setup_db();
        assert!(column_exists(
            &conn,
            "ideation_sessions",
            "team_config_json"
        ));
    }

    #[test]
    fn test_existing_rows_have_null_team_mode() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s-1', 'p-1', 'active')",
            [],
        )
        .unwrap();

        let team_mode: Option<String> = conn
            .query_row(
                "SELECT team_mode FROM ideation_sessions WHERE id = 's-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(team_mode.is_none());
    }

    #[test]
    fn test_insert_with_team_mode() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO ideation_sessions (id, project_id, status, team_mode, team_config_json)
             VALUES ('s-2', 'p-1', 'active', 'research', '{\"max_teammates\":3,\"model_ceiling\":\"sonnet\",\"budget_limit\":null,\"composition_mode\":\"auto\"}')",
            [],
        )
        .unwrap();

        let (mode, config): (String, String) = conn
            .query_row(
                "SELECT team_mode, team_config_json FROM ideation_sessions WHERE id = 's-2'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(mode, "research");
        assert!(config.contains("max_teammates"));
    }

    #[test]
    fn test_migration_is_idempotent() {
        let conn = setup_db();
        v38_ideation_team_mode::migrate(&conn).unwrap();
    }
}
