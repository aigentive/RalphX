use super::{run_migrations_through, v20260715183000_automation_ideation_signal};
use crate::infrastructure::sqlite::open_memory_connection;

#[test]
fn migration_widens_automation_signal_for_the_ideation_bridge() {
    let conn = open_memory_connection().expect("create memory db");
    run_migrations_through(&conn, 20260715170000).expect("run prior migrations");
    conn.execute(
        "INSERT INTO projects (id, name, working_directory)
         VALUES ('project-ideation-signal', 'Ideation Signal', '/tmp/ideation-signal')",
        [],
    )
    .expect("insert project");

    let insert_bridge = || {
        conn.execute(
            "INSERT INTO automations (
                id, project_id, name, status, provider_harness, model_id, run_mode,
                base_ref_kind, completion_signal, plan_deep_verification
             ) VALUES (
                'automation-ideation-signal',
                'project-ideation-signal',
                'Ideation Signal',
                'draft',
                'codex',
                'gpt-5.5',
                'ideation',
                'project_default',
                'ideation_finalized',
                1
             )",
            [],
        )
    };
    assert!(insert_bridge().is_err());

    v20260715183000_automation_ideation_signal::migrate(&conn)
        .expect("widen completion signal check");

    insert_bridge().expect("ideation completion signal should insert");
}
