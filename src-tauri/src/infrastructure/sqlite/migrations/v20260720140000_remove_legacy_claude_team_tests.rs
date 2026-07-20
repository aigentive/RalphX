use super::{
    helpers::{column_exists, table_exists},
    run_migrations_through, v20260720140000_remove_legacy_claude_team,
};
use crate::infrastructure::sqlite::open_memory_connection;

const PREVIOUS_SCHEMA_VERSION: i64 = 20260718182035;

#[test]
fn migration_removes_legacy_state_and_preserves_native_team_artifacts() {
    let conn = open_memory_connection().expect("create memory db");
    run_migrations_through(&conn, PREVIOUS_SCHEMA_VERSION).expect("run prior migrations");
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('project-1', 'Project', '/tmp/project')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, metadata)
         VALUES ('task-1', 'project-1', 'Task', '{\"agent_variant\":\"team\",\"kept\":true}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, title, team_mode, team_config_json)
         VALUES ('session-1', 'project-1', 'Session', 'research', '{}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, coordination_mode)
         VALUES ('conversation-1', 'ideation', 'session-1', 'legacy_claude_team')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artifacts
            (id, type, name, content_type, content_text, bucket_id, created_by, metadata_json)
         VALUES
            ('finding-1', 'verification_finding', 'Finding', 'text', 'retired', 'team-findings', 'team-lead', '{\"author\":\"team-lead\"}'),
            ('summary-1', 'team_summary', 'Summary', 'text', 'kept', 'team-findings', 'team-lead', '{\"author\":\"team-lead\"}')",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE artifacts SET previous_version_id = 'finding-1' WHERE id = 'summary-1'",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artifact_relations (id, from_artifact_id, to_artifact_id, relation_type)
         VALUES ('relation-1', 'summary-1', 'finding-1', 'references')",
        [],
    )
    .unwrap();

    v20260720140000_remove_legacy_claude_team::migrate(&conn).expect("remove legacy team state");
    v20260720140000_remove_legacy_claude_team::migrate(&conn)
        .expect("legacy cleanup is idempotent");

    assert!(!table_exists(&conn, "team_messages"));
    assert!(!table_exists(&conn, "team_sessions"));
    assert!(!column_exists(&conn, "ideation_sessions", "team_mode"));
    assert!(!column_exists(
        &conn,
        "ideation_sessions",
        "team_config_json"
    ));
    assert_eq!(
        conn.query_row(
            "SELECT coordination_mode FROM chat_conversations WHERE id = 'conversation-1'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "rx_native_team"
    );
    let metadata: String = conn
        .query_row(
            "SELECT metadata FROM tasks WHERE id = 'task-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(metadata, "{\"kept\":true}");
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM artifacts WHERE type = 'verification_finding'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    let preserved: (String, String, Option<String>) = conn
        .query_row(
            "SELECT created_by, metadata_json, previous_version_id FROM artifacts WHERE id = 'summary-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        preserved,
        ("system".into(), "{\"author\":\"system\"}".into(), None)
    );
    let bucket_config: String = conn
        .query_row(
            "SELECT config_json FROM artifact_buckets WHERE id = 'team-findings'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        bucket_config,
        "{\"accepted_types\":[\"team_research\",\"team_analysis\",\"team_summary\"],\"writers\":[\"system\"],\"readers\":[\"all\"]}"
    );
    let create_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'chat_conversations'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!create_sql.contains("legacy_claude_team"));
}

#[test]
fn migration_rolls_back_all_state_when_destructive_cleanup_fails() {
    let conn = open_memory_connection().expect("create memory db");
    run_migrations_through(&conn, PREVIOUS_SCHEMA_VERSION).expect("run prior migrations");
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('project-1', 'Project', '/tmp/project')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, coordination_mode)
         VALUES ('conversation-1', 'project', 'project-1', 'legacy_claude_team')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artifacts
            (id, type, name, content_type, content_text, created_by)
         VALUES ('finding-1', 'verification_finding', 'Finding', 'text', 'retired', 'system')",
        [],
    )
    .unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_finding_delete
         BEFORE DELETE ON artifacts
         WHEN OLD.type = 'verification_finding'
         BEGIN
             SELECT RAISE(ABORT, 'injected cleanup failure');
         END;",
    )
    .unwrap();

    let error = v20260720140000_remove_legacy_claude_team::migrate(&conn)
        .expect_err("injected failure must abort migration");
    assert!(error.to_string().contains("injected cleanup failure"));
    assert_eq!(
        conn.query_row(
            "SELECT coordination_mode FROM chat_conversations WHERE id = 'conversation-1'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "legacy_claude_team"
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM artifacts WHERE id = 'finding-1'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert!(table_exists(&conn, "team_messages"));
    assert!(table_exists(&conn, "team_sessions"));
    assert!(!table_exists(&conn, "chat_conversations_new_plan_mode"));
    assert!(!table_exists(&conn, "chat_conversations_old_plan_mode"));
}
