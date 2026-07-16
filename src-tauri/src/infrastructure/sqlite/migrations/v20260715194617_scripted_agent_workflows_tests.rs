//! Tests for migration v20260715194617: scripted agent workflows

use rusqlite::Connection;

use super::v20260715194617_scripted_agent_workflows;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

fn setup_parent_conversation(conn: &Connection) {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE chat_conversations (id TEXT PRIMARY KEY);
         CREATE TABLE delegated_sessions (id TEXT PRIMARY KEY);
         INSERT INTO chat_conversations (id) VALUES ('conversation-1');",
    )
    .expect("parent conversation fixture should be created");
}

#[test]
fn migration_creates_durable_workflow_lineage_tables() {
    let conn = setup_test_db();
    setup_parent_conversation(&conn);
    v20260715194617_scripted_agent_workflows::migrate(&conn)
        .expect("workflow migration should succeed");

    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name IN (
                'agent_workflow_scripts', 'agent_workflow_runs',
                'agent_workflow_phases', 'agent_workflow_invocations',
                'agent_workflow_logs'
             )",
            [],
            |row| row.get(0),
        )
        .expect("workflow tables should be queryable");
    assert_eq!(table_count, 5);
}

#[test]
fn script_edit_invalidates_hash_bound_approval() {
    let conn = setup_test_db();
    setup_parent_conversation(&conn);
    v20260715194617_scripted_agent_workflows::migrate(&conn)
        .expect("workflow migration should succeed");
    conn.execute(
        "INSERT INTO agent_workflow_scripts (
            id, conversation_id, project_id, name, description, script_source,
            script_hash, protocol_version, meta_json, permission_summary_json,
            permission_hash, estimated_fanout, approved_script_hash,
            approved_permission_hash, approved_at, created_at, updated_at
         ) VALUES (
            'script-1', 'conversation-1', 'project-1', 'Review', '', 'return 1',
            ?1, 1, '{}', '{}', ?2, 1, ?1, ?2,
            '2026-07-15T00:00:00Z', '2026-07-15T00:00:00Z', '2026-07-15T00:00:00Z'
         )",
        ["a".repeat(64), "b".repeat(64)],
    )
    .expect("approved script should insert");

    conn.execute(
        "UPDATE agent_workflow_scripts
         SET script_source = 'return 2', script_hash = ?1
         WHERE id = 'script-1'",
        ["c".repeat(64)],
    )
    .expect("script edit should succeed");

    let approval: (Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT approved_script_hash, approved_permission_hash, approved_at
             FROM agent_workflow_scripts WHERE id = 'script-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("script approval columns should be readable");
    assert_eq!(approval, (None, None, None));
}

#[test]
fn invocation_keys_are_unique_per_run_but_reusable_across_runs() {
    let conn = setup_test_db();
    setup_parent_conversation(&conn);
    v20260715194617_scripted_agent_workflows::migrate(&conn)
        .expect("workflow migration should succeed");
    let hash = "a".repeat(64);
    conn.execute(
        "INSERT INTO agent_workflow_scripts (
            id, conversation_id, project_id, name, description, script_source,
            script_hash, protocol_version, meta_json, permission_summary_json,
            permission_hash, estimated_fanout, created_at, updated_at
         ) VALUES (
            'script-1', 'conversation-1', 'project-1', 'Review', '', 'return 1',
            ?1, 1, '{}', '{}', ?1, 1,
            '2026-07-15T00:00:00Z', '2026-07-15T00:00:00Z'
         )",
        [hash.as_str()],
    )
    .expect("script should insert");
    for run_id in ["run-1", "run-2"] {
        conn.execute(
            "INSERT INTO agent_workflow_runs (
                id, script_id, conversation_id, project_id, harness,
                script_hash, permission_hash, args_json, status, attempt,
                pause_requested, cancel_requested, created_at, updated_at
             ) VALUES (
                ?1, 'script-1', 'conversation-1', 'project-1', 'codex',
                ?2, ?2, '{}', 'queued', 0, 0, 0,
                '2026-07-15T00:00:00Z', '2026-07-15T00:00:00Z'
             )",
            [run_id, hash.as_str()],
        )
        .expect("run should insert");
        conn.execute(
            "INSERT INTO agent_workflow_invocations (
                id, run_id, logical_key, agent_name, prompt_hash,
                status, created_at, updated_at
             ) VALUES (
                ?1, ?2, 'critic:0', 'ralphx-general-explorer', ?3,
                'pending', '2026-07-15T00:00:00Z', '2026-07-15T00:00:00Z'
             )",
            [
                format!("invocation-{run_id}"),
                run_id.to_string(),
                hash.clone(),
            ],
        )
        .expect("logical key should be reusable in a distinct run");
    }

    let duplicate = conn.execute(
        "INSERT INTO agent_workflow_invocations (
            id, run_id, logical_key, agent_name, prompt_hash,
            status, created_at, updated_at
         ) VALUES (
            'duplicate', 'run-1', 'critic:0', 'ralphx-general-explorer', ?1,
            'pending', '2026-07-15T00:00:00Z', '2026-07-15T00:00:00Z'
         )",
        [hash],
    );
    assert!(duplicate.is_err());
}
