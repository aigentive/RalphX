//! Tests for migration v20260722090527: project skill schema versioning

use rusqlite::Connection;

use super::{
    v20260614120000_learned_skill_substrate, v20260615092455_project_skill_settings,
    v20260722090527_project_skill_schema_versioning,
};

fn setup_pre_b1_db() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory database");
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA legacy_alter_table = OFF;
         CREATE TABLE projects (id TEXT PRIMARY KEY);
         INSERT INTO projects (id) VALUES ('project-1');",
    )
    .unwrap();
    v20260614120000_learned_skill_substrate::migrate(&conn).unwrap();
    v20260615092455_project_skill_settings::migrate(&conn).unwrap();
    conn.execute_batch(
        r#"INSERT INTO project_skills (
              id, project_id, title, bucket, stage, status, pinned, archived,
              scope_paths_json, compact_guidance, body_markdown, predicted_effect,
              provenance_json, companion_of_skill_id, created_at, updated_at
           ) VALUES (
              'parent', 'project-1', ' Review Rule ', 'REVIEW', 'review', 'approved', 1, 0,
              '["src-tauri"]', 'Check review.', 'Body', 'Avoid regressions',
              '{"source":"task_outcome","additional":{"pipeline_role":" verifier "}}',
              NULL, '2026-06-14T10:00:00+00:00', '2026-06-14T10:01:00+00:00'
           );
           INSERT INTO project_skills (
              id, project_id, title, bucket, stage, status, pinned, archived,
              scope_paths_json, compact_guidance, body_markdown, predicted_effect,
              provenance_json, companion_of_skill_id, created_at, updated_at
           ) VALUES (
              'child', 'project-1', 'Imported Rule', 'merge', 'merge', 'retired', 0, 1,
              '[]', 'Check merge.', 'Imported body', 'Avoid merge regressions',
              '{malformed', 'parent', '2026-06-14T11:00:00+00:00', '2026-06-14T11:01:00+00:00'
           );
           INSERT INTO skill_usage_events (
              id, project_id, project_skill_id, injection_kind, created_at
           ) VALUES ('usage-1', 'project-1', 'parent', 'explicit', '2026-06-14T12:00:00+00:00');
           INSERT INTO project_skill_settings (project_id, export_enabled)
           VALUES ('project-1', 1);"#,
    )
    .unwrap();
    conn
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .any(|name| name.unwrap() == column);
    exists
}

#[test]
fn migration_preserves_rows_links_indexes_and_backfills_b1_contract() {
    let conn = setup_pre_b1_db();
    v20260722090527_project_skill_schema_versioning::migrate(&conn).unwrap();

    assert_eq!(
        conn.query_row(
            "SELECT companion_of_skill_id FROM project_skills WHERE id = 'child'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "parent"
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM skill_usage_events", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'index' AND name IN (
               'idx_project_skills_project_status',
               'idx_project_skills_project_stage_bucket'
             )",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );

    let parent = conn
        .query_row(
            "SELECT version, content_hash, evidence_hash, created_by, pipeline_role
             FROM project_skills WHERE id = 'parent'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(parent.0, 1);
    assert_eq!(parent.1.len(), 64);
    assert_eq!(parent.2.len(), 64);
    assert_eq!(parent.3, "agent");
    assert_eq!(parent.4.as_deref(), Some("verifier"));

    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM project_skill_versions", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    let snapshot = conn
        .query_row(
            "SELECT title, body_markdown, provenance_json, status, content_hash
             FROM project_skill_versions WHERE project_skill_id = 'parent' AND version = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(snapshot.0, " Review Rule ");
    assert_eq!(snapshot.1, "Body");
    assert!(snapshot.2.contains("task_outcome"));
    assert_eq!(snapshot.3, "approved");
    assert_eq!(snapshot.4, parent.1);

    let settings = conn
        .query_row(
            "SELECT enabled, auto_inject, auto_distill, injection_max_skills,
                    injection_max_chars, injection_guidance_max_chars,
                    report_min_outcomes, verification_corpus_gate, export_enabled
             FROM project_skill_settings WHERE project_id = 'project-1'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(settings, (1, 0, 0, 4, 6_000, 400, 5, 0, 1));

    conn.execute(
        "INSERT INTO project_skills (
           id, project_id, title, bucket, stage, status, compact_guidance,
           body_markdown, predicted_effect, provenance_json, version,
           content_hash, evidence_hash, created_by
         ) VALUES (
           'stale', 'project-1', 'Stale', 'review', 'review', 'stale', 'Guide',
           'Body', 'Effect', '{}', 1, 'a', 'b', 'user'
         )",
        [],
    )
    .unwrap();
    assert!(conn
        .execute(
            "UPDATE project_skills SET status = 'unknown' WHERE id = 'stale'",
            [],
        )
        .is_err());
    assert!(conn
        .execute(
            "UPDATE project_skills SET created_by = 'system' WHERE id = 'stale'",
            []
        )
        .is_err());
    assert!(conn
        .execute(
            "UPDATE project_skills SET pipeline_role = '' WHERE id = 'stale'",
            []
        )
        .is_err());
    assert!(conn
        .execute(
            "UPDATE project_skill_settings SET injection_max_chars = 0",
            []
        )
        .is_err());
    assert!(conn
        .query_row("PRAGMA foreign_key_check", [], |row| row
            .get::<_, String>(0))
        .is_err());
}

#[test]
fn migration_is_idempotent_after_commit_without_duplicate_snapshots() {
    let conn = setup_pre_b1_db();
    v20260722090527_project_skill_schema_versioning::migrate(&conn).unwrap();

    conn.execute_batch(
        "UPDATE project_skills
         SET version = 2, title = 'Updated Rule', content_hash = 'v2-content',
             evidence_hash = 'v2-evidence', updated_at = '2026-06-14T10:02:00+00:00'
         WHERE id = 'parent';
         INSERT INTO project_skill_versions (
             project_skill_id, project_id, version, title, bucket, stage, status,
             pinned, archived, scope_paths_json, compact_guidance, body_markdown,
             predicted_effect, provenance_json, companion_of_skill_id, content_hash,
             evidence_hash, created_by, pipeline_role, skill_created_at,
             skill_updated_at, snapshot_created_at
         )
         SELECT id, project_id, version, title, bucket, stage, status,
                pinned, archived, scope_paths_json, compact_guidance, body_markdown,
                predicted_effect, provenance_json, companion_of_skill_id, content_hash,
                evidence_hash, created_by, pipeline_role, created_at, updated_at, updated_at
         FROM project_skills WHERE id = 'parent';",
    )
    .unwrap();

    v20260722090527_project_skill_schema_versioning::migrate(&conn).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM project_skill_versions", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        conn.query_row(
            "SELECT version, title, content_hash, evidence_hash
             FROM project_skills WHERE id = 'parent'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            )),
        )
        .unwrap(),
        (
            2,
            "Updated Rule".to_string(),
            "v2-content".to_string(),
            "v2-evidence".to_string(),
        )
    );
}

#[test]
fn migration_rolls_back_schema_and_data_and_restores_pragmas_on_failure() {
    let conn = setup_pre_b1_db();
    conn.execute_batch(
        "CREATE TRIGGER fail_project_skill_backfill
         BEFORE UPDATE ON project_skills
         BEGIN SELECT RAISE(ABORT, 'injected migration failure'); END;",
    )
    .unwrap();

    let error = v20260722090527_project_skill_schema_versioning::migrate(&conn)
        .expect_err("injected backfill must fail");
    assert!(error.to_string().contains("injected migration failure"));
    assert!(!column_exists(&conn, "project_skills", "version"));
    assert!(!column_exists(&conn, "project_skill_settings", "enabled"));
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM project_skills", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
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
