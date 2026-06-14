use rusqlite::Connection;

use super::v20260614120000_learned_skill_substrate;

#[test]
fn learned_skill_substrate_migration_creates_core_tables_and_dedupe_index() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON; CREATE TABLE projects (id TEXT PRIMARY KEY);")
        .unwrap();

    v20260614120000_learned_skill_substrate::migrate(&conn).unwrap();

    for table in ["task_outcomes", "project_skills", "skill_usage_events"] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "{table} table should exist");
    }

    conn.execute("INSERT INTO projects (id) VALUES ('project-1')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO task_outcomes (
            id, project_id, source, source_ref_kind, source_ref_id,
            outcome_class, status, evidence_json
        ) VALUES (
            'outcome-1', 'project-1', 'task_pipeline', 'task', 'task-1',
            NULL, 'unknown', '{}'
        )",
        [],
    )
    .unwrap();

    let duplicate = conn.execute(
        "INSERT INTO task_outcomes (
            id, project_id, source, source_ref_kind, source_ref_id,
            outcome_class, status, evidence_json
        ) VALUES (
            'outcome-2', 'project-1', 'task_pipeline', 'task', 'task-1',
            'known_class', 'eligible', '{}'
        )",
        [],
    );
    assert!(
        duplicate.is_err(),
        "source/ref dedupe must ignore outcome_class"
    );
}
