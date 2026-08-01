use rusqlite::params;

use super::{run_migrations_through, v20260707113000_automation_agent_completed_signal};
use crate::infrastructure::sqlite::open_memory_connection;

#[test]
fn migration_widens_automation_signal_and_run_status_checks() {
    let conn = open_memory_connection().expect("create memory db");
    run_migrations_through(&conn, 20260706113000).expect("run prior migrations");

    conn.execute(
        "INSERT INTO projects (id, name, working_directory)
         VALUES ('project-automation-signal', 'Automation Signal', '/tmp/automation-signal')",
        [],
    )
    .expect("insert project");
    assert!(conn
        .execute(
            "INSERT INTO automations (
                id, project_id, name, status, provider_harness, model_id, run_mode,
                base_ref_kind, completion_signal
             ) VALUES (
                'automation-signal-before',
                'project-automation-signal',
                'Automation Signal Before',
                'draft',
                'codex',
                'gpt-5.5',
                'plan',
                'project_default',
                'agent_completed'
             )",
            [],
        )
        .is_err());

    v20260707113000_automation_agent_completed_signal::migrate(&conn)
        .expect("widen automation signal");

    conn.execute(
        "INSERT INTO automations (
            id, project_id, name, status, provider_harness, model_id, run_mode,
            base_ref_kind, completion_signal
         ) VALUES (
            'automation-signal-after',
            'project-automation-signal',
            'Automation Signal After',
            'draft',
            'codex',
            'gpt-5.5',
            'plan',
            'project_default',
            'agent_completed'
         )",
        [],
    )
    .expect("agent_completed completion signal should insert");
    conn.execute(
        "INSERT INTO automation_runs (
            id, automation_id, run_index, status, run_prompt, prompt_author,
            base_ref_kind, base_ref_used
         ) VALUES (
            'automation-run-completed',
            ?1,
            1,
            'completed',
            'Plan the next slice',
            'setup_agent',
            'project_default',
            ''
         )",
        params!["automation-signal-after"],
    )
    .expect("completed automation run should insert");
}
