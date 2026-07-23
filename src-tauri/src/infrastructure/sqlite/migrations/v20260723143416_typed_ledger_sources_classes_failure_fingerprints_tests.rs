//! Tests for migration v20260723143416: typed ledger sources/classes and failure fingerprints.

use rusqlite::Connection;

use super::{
    v20260614120000_learned_skill_substrate, v20260723111500_project_skill_evidence_batches,
    v20260723143416_typed_ledger_sources_classes_failure_fingerprints,
};

fn setup_pre_d3_db() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory database");
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA legacy_alter_table = OFF;
         CREATE TABLE projects (id TEXT PRIMARY KEY);
         INSERT INTO projects (id) VALUES ('project-1');",
    )
    .unwrap();
    v20260614120000_learned_skill_substrate::migrate(&conn).unwrap();
    v20260723111500_project_skill_evidence_batches::migrate(&conn).unwrap();
    conn.execute_batch(
        r#"INSERT INTO project_skills (
              id, project_id, title, bucket, stage, status, compact_guidance,
              body_markdown, predicted_effect, provenance_json
           ) VALUES (
              'skill-1', 'project-1', 'Skill', 'merge', 'merge', 'approved',
              'Guidance', 'Body', 'Effect', '{}'
           );
           INSERT INTO task_outcomes (
              id, project_id, source, source_ref_kind, source_ref_id, task_id,
              outcome_class, status, evidence_json, created_at, updated_at
           ) VALUES
              ('live', 'project-1', 'merge', 'task', 'task-1', 'task-1',
               'merge_completed', 'succeeded', '{"live":true}',
               '2026-07-23T10:00:00+00:00', '2026-07-23T10:01:00+00:00'),
              ('compat', 'project-1', 'task_pipeline', 'task', 'task-2', 'task-2',
               'future_class', 'eligible', '{"compat":true}',
               '2026-07-23T10:02:00+00:00', '2026-07-23T10:03:00+00:00'),
              ('qa', 'project-1', 'qa', 'task', 'task-3', 'task-3',
               NULL, 'failed', '{"legacy":"qa"}',
               '2026-07-23T10:04:00+00:00', '2026-07-23T10:05:00+00:00'),
              ('workspace-review', 'project-1', 'workspace_review', 'review', 'review-1', NULL,
               '', 'unknown', '{"legacy":"workspace_review"}',
               '2026-07-23T10:06:00+00:00', '2026-07-23T10:07:00+00:00'),
              ('ideation', 'project-1', 'ideation', 'session', 'session-1', NULL,
               NULL, 'eligible', '{"legacy":"ideation"}',
               '2026-07-23T10:08:00+00:00', '2026-07-23T10:09:00+00:00');
           INSERT INTO skill_usage_events (
              id, project_id, project_skill_id, injection_kind, outcome_id,
              metadata_json, created_at
           ) VALUES (
              'usage-1', 'project-1', 'skill-1', 'explicit', 'live',
              '{"legacy":true}', '2026-07-23T10:10:00+00:00'
           );
           INSERT INTO project_skill_evidence_batches (
              id, project_id, fingerprint, bucket, status, created_at, updated_at
           ) VALUES (
              'batch-1', 'project-1',
              'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
              'execution', 'pending', '2026-07-23T10:11:00+00:00',
              '2026-07-23T10:11:00+00:00'
           );
           INSERT INTO project_skill_evidence_batch_items (
              batch_id, outcome_id, ordinal, digest
           ) VALUES ('batch-1', 'compat', 0, 'compat evidence');"#,
    )
    .unwrap();
    conn
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    conn.prepare(&format!("PRAGMA table_info({table})"))
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .any(|name| name.unwrap() == column)
}

#[test]
fn migration_preserves_rows_links_and_maps_only_known_legacy_values() {
    let conn = setup_pre_d3_db();
    v20260723143416_typed_ledger_sources_classes_failure_fingerprints::migrate(&conn).unwrap();

    let rows = conn
        .prepare(
            "SELECT id, source, outcome_class, failure_fingerprint
             FROM task_outcomes ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            (
                "compat".to_string(),
                "task_pipeline".to_string(),
                Some("future_class".to_string()),
                None,
            ),
            ("ideation".to_string(), "plan_mode".to_string(), None, None,),
            (
                "live".to_string(),
                "merge".to_string(),
                Some("merge_completed".to_string()),
                None,
            ),
            ("qa".to_string(), "verification".to_string(), None, None,),
            (
                "workspace-review".to_string(),
                "review".to_string(),
                Some(String::new()),
                None,
            ),
        ]
    );
    assert_eq!(
        conn.query_row(
            "SELECT injection_kind, outcome_id, metadata_json
             FROM skill_usage_events WHERE id = 'usage-1'",
            [],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        )
        .unwrap(),
        (
            "full_load".to_string(),
            "live".to_string(),
            r#"{"legacy":true}"#.to_string(),
        )
    );
    assert_eq!(
        conn.query_row(
            "SELECT outcome_id FROM project_skill_evidence_batch_items
             WHERE batch_id = 'batch-1'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "compat"
    );
    assert!(conn
        .query_row("PRAGMA foreign_key_check", [], |row| row
            .get::<_, String>(0))
        .is_err());
}

#[test]
fn migration_enforces_source_and_usage_vocabularies_but_keeps_classes_open() {
    let conn = setup_pre_d3_db();
    v20260723143416_typed_ledger_sources_classes_failure_fingerprints::migrate(&conn).unwrap();

    let task_outcomes_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'task_outcomes'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for source in [
        "agent_session",
        "agent_workspace",
        "agent_workspace_pr",
        "github_pr_review",
        "agent_conversation",
        "review",
        "git_commit_history",
        "github_pr_history",
        "plan_mode",
        "merge",
        "merge_validation",
        "verification",
        "task_pipeline",
    ] {
        assert!(task_outcomes_sql.contains(&format!("'{source}'")));
    }
    assert!(task_outcomes_sql.contains("failure_fingerprint"));
    assert!(!task_outcomes_sql.contains("outcome_class IN"));

    assert!(conn
        .execute(
            "INSERT INTO task_outcomes (
               id, project_id, source, source_ref_kind, source_ref_id,
               outcome_class, status, evidence_json
             ) VALUES (
               'invalid-source', 'project-1', 'qa', 'task', 'invalid',
               'anything', 'failed', '{}'
             )",
            [],
        )
        .is_err());
    conn.execute(
        "INSERT INTO task_outcomes (
           id, project_id, source, source_ref_kind, source_ref_id,
           outcome_class, status, evidence_json
         ) VALUES (
           'future-class', 'project-1', 'verification', 'task', 'future',
           'future_unrestricted_class', 'failed', '{}'
         )",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE task_outcomes SET failure_fingerprint = ?1 WHERE id = 'future-class'",
        ["a".repeat(64)],
    )
    .unwrap();
    assert!(conn
        .execute(
            "UPDATE task_outcomes SET failure_fingerprint = 'ABC' WHERE id = 'future-class'",
            [],
        )
        .is_err());

    for (index, kind) in [
        "compact_index",
        "full_load",
        "composer_directive",
        "interactive_stdin_unattributed",
    ]
    .into_iter()
    .enumerate()
    {
        conn.execute(
            "INSERT INTO skill_usage_events (
               id, project_id, project_skill_id, injection_kind
             ) VALUES (?1, 'project-1', 'skill-1', ?2)",
            rusqlite::params![format!("kind-usage-{index}"), kind],
        )
        .unwrap();
    }
    assert!(conn
        .execute(
            "INSERT INTO skill_usage_events (
               id, project_id, project_skill_id, injection_kind
             ) VALUES ('invalid-usage', 'project-1', 'skill-1', 'explicit')",
            [],
        )
        .is_err());

    for index in [
        "idx_task_outcomes_source_ref",
        "idx_task_outcomes_project_status",
        "idx_task_outcomes_failure_fingerprint",
    ] {
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
                [index],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1,
            "{index} must be recreated"
        );
    }
}

#[test]
fn migration_rolls_back_unknown_legacy_values_and_restores_pragmas() {
    for (column, value) in [
        ("source", "unknown_source"),
        ("injection_kind", "unknown_kind"),
    ] {
        let conn = setup_pre_d3_db();
        let sql = if column == "source" {
            "UPDATE task_outcomes SET source = ?1 WHERE id = 'live'"
        } else {
            "UPDATE skill_usage_events SET injection_kind = ?1 WHERE id = 'usage-1'"
        };
        conn.execute(sql, [value]).unwrap();

        let error =
            v20260723143416_typed_ledger_sources_classes_failure_fingerprints::migrate(&conn)
                .expect_err("unknown legacy values must fail closed");
        assert!(
            error.to_string().contains("CHECK constraint failed"),
            "unexpected error: {error}"
        );
        assert!(!column_exists(
            &conn,
            "task_outcomes",
            "failure_fingerprint"
        ));
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM task_outcomes", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            5
        );
        assert_eq!(
            conn.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("PRAGMA legacy_alter_table", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
