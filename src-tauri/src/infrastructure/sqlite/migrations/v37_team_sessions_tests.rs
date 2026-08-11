// Tests for v37 team_sessions migration

#[cfg(test)]
mod tests {
    use crate::infrastructure::sqlite::migrations::run_migrations_through;
    use crate::infrastructure::sqlite::migrations::v37_team_sessions;
    use crate::infrastructure::sqlite::open_connection;
    use std::path::PathBuf;

    fn setup_db() -> rusqlite::Connection {
        let conn = open_connection(&PathBuf::from(":memory:")).unwrap();
        run_migrations_through(&conn, 37).unwrap();
        conn
    }

    #[test]
    fn test_team_sessions_table_created() {
        let conn = setup_db();
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='team_sessions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_team_messages_table_created() {
        let conn = setup_db();
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='team_messages'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_insert_team_session() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO team_sessions (id, team_name, context_id, context_type, lead_name, phase, teammate_json)
             VALUES ('ts-1', 'alpha-team', 'session-123', 'ideation', 'researcher', 'active', '[{\"name\":\"researcher\"}]')",
            [],
        )
        .unwrap();

        let name: String = conn
            .query_row(
                "SELECT team_name FROM team_sessions WHERE id = 'ts-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "alpha-team");
    }

    #[test]
    fn test_insert_team_message() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO team_sessions (id, team_name, context_id, context_type, phase)
             VALUES ('ts-1', 'team1', 'ctx-1', 'ideation', 'active')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO team_messages (id, team_session_id, sender, recipient, content, message_type)
             VALUES ('msg-1', 'ts-1', 'researcher', 'planner', 'Hello', 'teammate_message')",
            [],
        )
        .unwrap();

        let sender: String = conn
            .query_row(
                "SELECT sender FROM team_messages WHERE id = 'msg-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(sender, "researcher");
    }

    #[test]
    fn test_team_findings_bucket_seeded() {
        let conn = setup_db();
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM artifact_buckets WHERE id = 'team-findings'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_migration_is_idempotent() {
        let conn = setup_db();
        // Run migration again — should not fail
        v37_team_sessions::migrate(&conn).unwrap();
    }
}
