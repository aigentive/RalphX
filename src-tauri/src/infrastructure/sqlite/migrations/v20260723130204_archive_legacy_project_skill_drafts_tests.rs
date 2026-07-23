//! Tests for migration v20260723130204: archive legacy project skill drafts

use rusqlite::Connection;

use super::v20260723130204_archive_legacy_project_skill_drafts;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE project_skills (
             id TEXT PRIMARY KEY,
             status TEXT NOT NULL,
             archived INTEGER NOT NULL DEFAULT 0,
             pinned INTEGER NOT NULL DEFAULT 0,
             provenance_json TEXT NOT NULL DEFAULT '{}',
             updated_at TEXT NOT NULL
         );
         CREATE TABLE project_skill_versions (
             id TEXT PRIMARY KEY,
             project_skill_id TEXT NOT NULL REFERENCES project_skills(id)
         );",
    )
    .expect("create project-skill migration fixture");
    conn
}

#[test]
fn archives_only_exact_staged_legacy_drafts_and_is_idempotent() {
    let conn = setup_test_db();
    let rows = [
        (
            "deterministic",
            "staged",
            0,
            r#"{"additional":{"distiller":"deterministic_eligible_outcome_v1"}}"#,
        ),
        (
            "git-history",
            "staged",
            0,
            r#"{"additional":{"distiller":"git_history_commit_v1"}}"#,
        ),
        (
            "direct-pr",
            "staged",
            0,
            r#"{"source":"github_pr_history","authoring_contract":"project-skill-authoring"}"#,
        ),
        (
            "approved-legacy",
            "approved",
            0,
            r#"{"additional":{"distiller":"deterministic_eligible_outcome_v1"}}"#,
        ),
        (
            "manual",
            "staged",
            0,
            r#"{"source":"project_skill_import","authoring_contract":"project-skill-authoring"}"#,
        ),
        (
            "pipeline",
            "staged",
            0,
            r#"{"source":"skill_pipeline_mcp","additional":{"pipeline_role":"skill_distiller"}}"#,
        ),
    ];
    for (id, status, archived, provenance) in rows {
        conn.execute(
            "INSERT INTO project_skills (
                id, status, archived, pinned, provenance_json, updated_at
             ) VALUES (?1, ?2, ?3, 0, ?4, '2026-01-01T00:00:00+00:00')",
            rusqlite::params![id, status, archived, provenance],
        )
        .expect("seed project skill");
    }
    conn.execute(
        "INSERT INTO project_skill_versions (id, project_skill_id)
         VALUES ('version-1', 'deterministic')",
        [],
    )
    .expect("seed project skill version");

    v20260723130204_archive_legacy_project_skill_drafts::migrate(&conn).expect("first migration");
    v20260723130204_archive_legacy_project_skill_drafts::migrate(&conn).expect("idempotent rerun");

    for id in ["deterministic", "git-history", "direct-pr"] {
        let state = conn
            .query_row(
                "SELECT status, archived, pinned FROM project_skills WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .expect("read archived legacy draft");
        assert_eq!(state, ("archived".to_string(), 1, 0));
    }
    assert_eq!(
        conn.query_row(
            "SELECT status FROM project_skills WHERE id = 'approved-legacy'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("read approved legacy skill"),
        "approved"
    );
    for id in ["manual", "pipeline"] {
        let state = conn
            .query_row(
                "SELECT status, archived FROM project_skills WHERE id = ?1",
                [id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .expect("read preserved skill");
        assert_eq!(state, ("staged".to_string(), 0));
    }
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM project_skill_versions WHERE project_skill_id = 'deterministic'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("count preserved versions"),
        1
    );
}
