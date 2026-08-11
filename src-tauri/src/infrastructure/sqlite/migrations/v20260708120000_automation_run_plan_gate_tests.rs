use rusqlite::Connection;

use crate::domain::entities::{is_open_automation_run, AutomationJudgeState, AutomationRunStatus};

use super::{
    v20260704193000_automations_p1, v20260707113000_automation_agent_completed_signal,
    v20260707120000_automations_spec_artifact_id, v20260708120000_automation_run_plan_gate,
};

fn setup_previous_automation_schema() -> Connection {
    let conn = Connection::open_in_memory().expect("create in-memory database");
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;

         CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            working_directory TEXT NOT NULL
         );

         CREATE TABLE chat_conversations (
            id TEXT PRIMARY KEY,
            context_type TEXT NOT NULL,
            context_id TEXT NOT NULL,
            agent_mode TEXT CHECK(agent_mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr'))
         );

         CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            mode TEXT NOT NULL CHECK (mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr')),
            base_ref_kind TEXT NOT NULL,
            base_ref TEXT NOT NULL,
            branch_name TEXT NOT NULL,
            worktree_path TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived', 'missing')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(conversation_id) REFERENCES chat_conversations(id) ON DELETE CASCADE
         );

         INSERT INTO projects (id, name, working_directory)
         VALUES ('project-1', 'Project 1', '/tmp/project-1');",
    )
    .expect("create base schema");
    v20260704193000_automations_p1::migrate(&conn).expect("create automations schema");
    v20260707113000_automation_agent_completed_signal::migrate(&conn)
        .expect("add completed signal status");
    v20260707120000_automations_spec_artifact_id::migrate(&conn).expect("add spec artifact id");
    conn
}

fn insert_automation(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO automations (
            id, project_id, name, status, provider_harness, model_id, run_mode, base_ref_kind
         ) VALUES (
            ?1, 'project-1', 'Automation', 'active', 'claude', 'sonnet', 'edit', 'project_default'
         )",
        [id],
    )
    .expect("insert automation");
}

#[test]
fn migration_adds_plan_gate_columns_defaults_and_status_check() {
    let conn = setup_previous_automation_schema();
    insert_automation(&conn, "automation-existing");
    conn.execute(
        "INSERT INTO automation_runs (
            id, automation_id, run_index, status, run_prompt, prompt_author, base_ref_kind, base_ref_used
         ) VALUES (
            'run-existing', 'automation-existing', 1, 'running', 'Prompt', 'setup_agent', 'project_default', ''
         )",
        [],
    )
    .expect("insert existing run before migration");

    v20260708120000_automation_run_plan_gate::migrate(&conn)
        .expect("add automation run plan gate fields");

    let automation_defaults: (String, String, i64) = conn
        .query_row(
            "SELECT plan_approval_mode, pr_merge_mode, plan_deep_verification
             FROM automations
             WHERE id = 'automation-existing'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        automation_defaults,
        ("manual".to_string(), "manual".to_string(), 0)
    );

    let run_defaults: (
        String,
        Option<String>,
        Option<String>,
        i64,
        i64,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT plan_judge_state, plan_judge_lease_expires_at, plan_judge_verdict_json,
                    plan_revision_round, plan_reminder_count, plan_pending_instructions,
                    plan_last_parked_artifact_id, agent_phase_started_at
             FROM automation_runs
             WHERE id = 'run-existing'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        run_defaults,
        ("none".to_string(), None, None, 0, 0, None, None, None)
    );
    let preserved_run_values: (String, String) = conn
        .query_row(
            "SELECT status, run_prompt
             FROM automation_runs
             WHERE id = 'run-existing'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        preserved_run_values,
        ("running".to_string(), "Prompt".to_string())
    );

    insert_automation(&conn, "automation-awaiting");
    conn.execute(
        "INSERT INTO automation_runs (
            id, automation_id, run_index, status, run_prompt, prompt_author, base_ref_kind, base_ref_used
         ) VALUES (
            'run-awaiting', 'automation-awaiting', 1, 'awaiting_plan_approval', 'Prompt', 'setup_agent', 'project_default', ''
         )",
        [],
    )
    .expect("new status should satisfy the rebuilt CHECK");
    assert!(conn
        .execute(
            "INSERT INTO automation_runs (
                id, automation_id, run_index, status, run_prompt, prompt_author, base_ref_kind, base_ref_used
             ) VALUES (
                'run-invalid', 'automation-awaiting', 2, 'plan_approved', 'Prompt', 'setup_agent', 'project_default', ''
             )",
            [],
        )
        .is_err());
}

#[test]
fn migration_recreates_single_open_index_with_awaiting_plan_approval() {
    let conn = setup_previous_automation_schema();
    v20260708120000_automation_run_plan_gate::migrate(&conn)
        .expect("add automation run plan gate fields");

    let index_sql: String = conn
        .query_row(
            "SELECT sql
             FROM sqlite_master
             WHERE type = 'index'
               AND name = 'idx_automation_runs_single_open'",
            [],
            |row| row.get(0),
        )
        .expect("single-open index should exist");
    assert!(
        index_sql.contains("awaiting_plan_approval"),
        "index SQL must include parked plan-gate runs: {index_sql}"
    );

    insert_automation(&conn, "automation-single-open");
    conn.execute(
        "INSERT INTO automation_runs (
            id, automation_id, run_index, status, run_prompt, prompt_author, base_ref_kind, base_ref_used
         ) VALUES (
            'run-parked', 'automation-single-open', 1, 'awaiting_plan_approval', 'Prompt', 'setup_agent', 'project_default', ''
         )",
        [],
    )
    .expect("parked run should insert as the open run");
    assert!(
        conn.execute(
            "INSERT INTO automation_runs (
                id, automation_id, run_index, status, run_prompt, prompt_author, base_ref_kind, base_ref_used
             ) VALUES (
                'run-duplicate-open', 'automation-single-open', 2, 'pending', 'Prompt', 'setup_agent', 'project_default', ''
             )",
            [],
        )
        .is_err(),
        "parked run must participate in the single-open invariant"
    );
}

#[test]
fn automation_run_status_lockstep_matches_domain_index() {
    use AutomationJudgeState::{Done, Failed, InProgress, None, Skipped};
    use AutomationRunStatus::{
        AgentFailed, AwaitingPlanApproval, Cancelled, Completed, Merged, Pending, PrClosed,
        Provisioning, Published, Running,
    };

    let statuses = [
        Pending,
        Provisioning,
        Running,
        AwaitingPlanApproval,
        Published,
        Completed,
        Merged,
        PrClosed,
        AgentFailed,
        Cancelled,
    ];
    let judge_states = [None, InProgress, Done, Failed, Skipped];

    for status in statuses {
        for judge_state in judge_states {
            let index_open = v20260708120000_automation_run_plan_gate::single_open_index_includes(
                status.as_str(),
                judge_state.as_str(),
            );
            assert_eq!(
                is_open_automation_run(status, judge_state),
                index_open,
                "domain and SQLite index disagree for {status:?}/{judge_state:?}"
            );
        }
    }
}
