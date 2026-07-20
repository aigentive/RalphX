//! Tests for migration v20260717152714: persona artifact history

use rusqlite::Connection;

use super::{run_migrations_through, v20260717152714_persona_artifact_history};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn migration_backfills_persona_artifacts_with_user_version_and_tip() {
    let conn = setup_test_db();
    run_migrations_through(&conn, 20260717152713).expect("binding migrations should succeed");
    conn.execute(
        "INSERT INTO personas (
            id, slug, name, description, content, status, version, content_hash,
            source_json, created_at, updated_at
         ) VALUES ('persona-one', 'reviewer', 'Reviewer', '', 'persona content',
                   'active', 7, 'hash', '{}', '2026-07-17T00:00:00+00:00',
                   '2026-07-17T00:00:00+00:00')",
        [],
    )
    .expect("persona fixture should seed");

    v20260717152714_persona_artifact_history::migrate(&conn)
        .expect("artifact migration should succeed");

    let artifact_id: String = conn
        .query_row(
            "SELECT artifact_id FROM personas WHERE id = 'persona-one'",
            [],
            |row| row.get(0),
        )
        .expect("persona tip should load");
    let artifact: (String, String, String, String, i64) = conn
        .query_row(
            "SELECT type, content_text, created_by, metadata_json, version
             FROM artifacts WHERE id = ?1",
            [artifact_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("backfilled artifact should load");
    assert_eq!(artifact.0, "persona");
    assert_eq!(artifact.1, "persona content");
    assert_eq!(artifact.2, "backfill");
    assert_eq!(artifact.4, 1, "chain version starts independently at one");
    let metadata: serde_json::Value =
        serde_json::from_str(&artifact.3).expect("metadata should be JSON");
    assert_eq!(metadata["persona_version"], 7);
    assert_eq!(metadata["created_by"], "backfill");

    let bucket_config: String = conn
        .query_row(
            "SELECT config_json FROM artifact_buckets WHERE id = 'persona-library'",
            [],
            |row| row.get(0),
        )
        .expect("persona bucket should exist");
    assert!(bucket_config.contains("persona"));
}

#[test]
fn migration_converges_after_artifact_insert_without_persona_tip_update() {
    let conn = setup_test_db();
    run_migrations_through(&conn, 20260717152713).expect("binding migrations should succeed");
    conn.execute(
        "INSERT INTO personas (
            id, slug, name, description, content, status, version, content_hash,
            source_json, created_at, updated_at
         ) VALUES ('persona-partial', 'reviewer', 'Reviewer', '', 'partial content',
                   'active', 4, 'hash', '{}', '2026-07-17T00:00:00+00:00',
                   '2026-07-17T00:00:00+00:00')",
        [],
    )
    .expect("persona fixture should seed");
    conn.execute_batch(
        "ALTER TABLE personas ADD COLUMN artifact_id TEXT NULL;
         INSERT INTO artifact_buckets (id, name, config_json, is_system)
         VALUES (
             'persona-library',
             'Persona Library',
             '{\"accepted_types\":[\"persona\"],\"writers\":[\"agent\",\"user\",\"system\"],\"readers\":[\"all\"]}',
             1
         );
         INSERT INTO artifacts (
             id, type, name, content_type, content_text, bucket_id, created_by,
             version, metadata_json, created_at
         ) VALUES (
             'persona-artifact-persona-partial', 'persona', 'Reviewer', 'inline',
             'partial content', 'persona-library', 'backfill', 1,
             '{\"persona_version\":4,\"created_by\":\"backfill\"}',
             '2026-07-17T00:00:00+00:00'
         );",
    )
    .expect("partial migration state should seed");

    v20260717152714_persona_artifact_history::migrate(&conn)
        .expect("migration rerun should converge");

    let artifact_id: String = conn
        .query_row(
            "SELECT artifact_id FROM personas WHERE id = 'persona-partial'",
            [],
            |row| row.get(0),
        )
        .expect("persona tip should converge");
    assert_eq!(artifact_id, "persona-artifact-persona-partial");
    let artifact_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE id = 'persona-artifact-persona-partial'",
            [],
            |row| row.get(0),
        )
        .expect("artifact count should load");
    assert_eq!(artifact_count, 1, "rerun must not duplicate the artifact");
}
