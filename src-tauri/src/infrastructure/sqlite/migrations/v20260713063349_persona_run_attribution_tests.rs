use rusqlite::Connection;

use super::v20260713063349_persona_run_attribution;

fn setup_persona_agent_runs_table() -> Connection {
    let conn = Connection::open_in_memory().expect("create persona migration database");
    conn.execute_batch(
        "CREATE TABLE agent_runs (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            status TEXT NOT NULL
        );",
    )
    .expect("create pre-migration agent_runs table");
    conn
}

#[test]
fn persona_run_attribution_migration_adds_body_free_columns() {
    let conn = setup_persona_agent_runs_table();
    v20260713063349_persona_run_attribution::migrate(&conn).unwrap();

    let mut statement = conn.prepare("PRAGMA table_info(agent_runs)").unwrap();
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    for expected in [
        "persona_id",
        "persona_slug",
        "persona_version",
        "persona_content_hash",
        "persona_injected",
        "persona_skipped_reason",
    ] {
        assert!(columns.iter().any(|column| column == expected));
    }
    assert!(!columns.iter().any(|column| column == "persona_body"));
    assert!(!columns.iter().any(|column| column == "persona_content"));
}

#[test]
fn persona_run_attribution_migration_is_idempotent() {
    let conn = setup_persona_agent_runs_table();

    v20260713063349_persona_run_attribution::migrate(&conn).unwrap();
    v20260713063349_persona_run_attribution::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('agent_runs')
             WHERE name LIKE 'persona_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 6);
}

#[test]
fn persona_run_attribution_migration_leaves_existing_rows_null() {
    let conn = setup_persona_agent_runs_table();
    conn.execute(
        "INSERT INTO agent_runs (id, conversation_id, status) VALUES (?1, ?2, ?3)",
        ["run-before-persona", "conversation-1", "completed"],
    )
    .unwrap();

    v20260713063349_persona_run_attribution::migrate(&conn).unwrap();

    let values: (
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT persona_id, persona_slug, persona_version, persona_content_hash,
                    persona_injected, persona_skipped_reason
             FROM agent_runs WHERE id = 'run-before-persona'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(values, (None, None, None, None, None, None));
}
