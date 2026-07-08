//! Tests for migration v20260707120000: automations spec artifact id

use super::{run_migrations_through, v20260707120000_automations_spec_artifact_id};
use crate::infrastructure::sqlite::open_memory_connection;

#[test]
fn migration_adds_usable_spec_artifact_id_column() {
    let conn = open_memory_connection().expect("create memory db");
    run_migrations_through(&conn, 20260707113000).expect("run prior migrations");

    conn.execute(
        "INSERT INTO projects (id, name, working_directory)
         VALUES ('project-spec-linkage', 'Spec Linkage', '/tmp/spec-linkage')",
        [],
    )
    .expect("insert project");
    conn.execute(
        "INSERT INTO automations (
            id, project_id, name, status, provider_harness, model_id, run_mode,
            base_ref_kind, completion_signal
         ) VALUES (
            'automation-spec-before',
            'project-spec-linkage',
            'Automation Spec Before',
            'draft',
            'claude',
            'sonnet',
            'edit',
            'project_default',
            'pr_merged'
         )",
        [],
    )
    .expect("insert automation");

    v20260707120000_automations_spec_artifact_id::migrate(&conn).expect("add spec_artifact_id");

    conn.execute(
        "UPDATE automations SET spec_artifact_id = 'artifact-1' WHERE id = 'automation-spec-before'",
        [],
    )
    .expect("spec_artifact_id column should be writable");

    let stored: Option<String> = conn
        .query_row(
            "SELECT spec_artifact_id FROM automations WHERE id = 'automation-spec-before'",
            [],
            |row| row.get(0),
        )
        .expect("read spec_artifact_id");
    assert_eq!(stored.as_deref(), Some("artifact-1"));
}
