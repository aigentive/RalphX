use rusqlite::{params, Connection};

use super::v20260704193000_automations_p1;

fn setup_review_pr_mode_schema() -> Connection {
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
    conn
}

#[test]
fn migration_creates_automation_schema_and_widens_modes() {
    let conn = setup_review_pr_mode_schema();

    assert!(conn
        .execute(
            "INSERT INTO chat_conversations (id, context_type, context_id, agent_mode)
             VALUES ('setup-before', 'project', 'project-1', 'automation')",
            [],
        )
        .is_err());

    v20260704193000_automations_p1::migrate(&conn).expect("migrate automations schema");

    conn.execute(
        "INSERT INTO chat_conversations (
            id, context_type, context_id, agent_mode, automation_id, automation_run_id
         ) VALUES (
            'setup-after', 'project', 'project-1', 'automation', NULL, NULL
         )",
        [],
    )
    .expect("automation setup conversation mode should insert");
    conn.execute(
        "INSERT INTO agent_conversation_workspaces (
            conversation_id,
            project_id,
            mode,
            base_ref_kind,
            base_ref,
            branch_name,
            worktree_path,
            created_at,
            updated_at
        )
        VALUES (
            'setup-after',
            'project-1',
            'automation',
            'local_branch',
            'main',
            'ralphx/project-1/agent-setup-after',
            '/tmp/agent-setup-after',
            '2026-07-04T19:30:00+00:00',
            '2026-07-04T19:30:00+00:00'
        )",
        [],
    )
    .expect("automation workspace mode should insert");

    conn.execute(
        "INSERT INTO automations (
            id, project_id, name, status, provider_harness, model_id, run_mode, base_ref_kind
         ) VALUES (
            'automation-1', 'project-1', 'Automation 1', 'draft', 'claude', 'sonnet', 'edit', 'project_default'
         )",
        [],
    )
    .expect("automation row should insert");

    conn.execute(
        "INSERT INTO automation_runs (
            id, automation_id, run_index, status, run_prompt, prompt_author, base_ref_kind, base_ref_used
         ) VALUES (
            'run-1', 'automation-1', 1, 'agent_failed', 'Prompt', 'setup_agent', 'project_default', ''
         )",
        [],
    )
    .expect("unjudged failed run should insert");

    let duplicate = conn.execute(
        "INSERT INTO automation_runs (
            id, automation_id, run_index, status, run_prompt, prompt_author, base_ref_kind, base_ref_used
         ) VALUES (
            'run-2', 'automation-1', 2, 'pending', 'Prompt 2', 'judge', 'local_branch', 'main'
         )",
        [],
    );
    assert!(
        duplicate.is_err(),
        "partial unique index must reject a successor while failed/unjudged run is open"
    );

    conn.execute(
        "UPDATE automation_runs SET judge_state = 'done' WHERE id = 'run-1'",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO automation_runs (
            id, automation_id, run_index, status, run_prompt, prompt_author, base_ref_kind, base_ref_used
         ) VALUES (
            'run-2', 'automation-1', 2, 'pending', 'Prompt 2', 'judge', 'local_branch', 'main'
         )",
        [],
    )
    .expect("successor should insert after failed run is judged");
}

#[test]
fn migration_cascades_automation_children() {
    let conn = setup_review_pr_mode_schema();
    v20260704193000_automations_p1::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO automations (
            id, project_id, name, status, provider_harness, model_id, run_mode, base_ref_kind
         ) VALUES (
            'automation-cascade', 'project-1', 'Automation Cascade', 'draft', 'claude', 'sonnet', 'edit', 'project_default'
         )",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO automation_attachments (
            id, automation_id, file_name, file_path, created_at
         ) VALUES (
            'attachment-1',
            'automation-cascade',
            'spec.md',
            '/tmp/app-data/automation-attachments/a/b/spec.md',
            '2026-07-04T19:30:00+00:00'
         )",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO automation_context_refs (
            id, automation_id, ref_kind, payload_json, position
         ) VALUES (
            'ref-1', 'automation-cascade', 'project', '{}', 0
         )",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO automation_runs (
            id, automation_id, run_index, status, run_prompt, prompt_author, base_ref_kind, base_ref_used
         ) VALUES (
            'run-cascade', 'automation-cascade', 1, 'pending', 'Prompt', 'setup_agent', 'project_default', ''
         )",
        [],
    )
    .unwrap();

    conn.execute(
        "DELETE FROM automations WHERE id = ?1",
        params!["automation-cascade"],
    )
    .unwrap();

    for table in [
        "automation_runs",
        "automation_attachments",
        "automation_context_refs",
    ] {
        let count: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE automation_id = 'automation-cascade'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "{table} should cascade delete");
    }
}
