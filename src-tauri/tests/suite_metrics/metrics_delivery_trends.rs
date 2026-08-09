use rusqlite::Connection;

use ralphx_lib::commands::metrics_commands::compute_project_trends;

fn create_schema(conn: &Connection) {
    conn.execute_batch(
        "
        CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            internal_status TEXT NOT NULL,
            archived_at TEXT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE task_state_history (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            to_status TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            linked_plan_branch_id TEXT NULL,
            publication_pr_number INTEGER NULL,
            publication_pr_status TEXT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE agent_conversation_workspace_publication_events (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            step TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE plan_branches (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            merge_task_id TEXT NULL,
            pr_number INTEGER NULL,
            pr_status TEXT NULL,
            merged_at TEXT NULL,
            last_polled_at TEXT NULL
        );
        ",
    )
    .expect("create schema");
}

#[test]
fn weekly_delivery_throughput_dedupes_task_pipeline_and_direct_workspace_output() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);

    // Use relative dates so the events stay within the 77-day rolling window regardless of
    // when the test runs.
    conn.execute_batch(
        "INSERT INTO tasks (id, project_id, internal_status, created_at, updated_at)
         VALUES ('task-1', 'proj-1', 'merged',
                 datetime('now', '-10 days'),
                 datetime('now', '-8 days'));

         INSERT INTO task_state_history (id, task_id, to_status, created_at)
         VALUES ('hist-ready',  'task-1', 'ready',  datetime('now', '-10 days')),
                ('hist-merged', 'task-1', 'merged', datetime('now', '-8 days'));

         INSERT INTO plan_branches (
             id, project_id, merge_task_id, pr_number, pr_status, merged_at, last_polled_at
         ) VALUES
             ('pb-1',      'proj-1', 'task-1', 10, 'merged', datetime('now', '-8 days'), datetime('now', '-8 days')),
             ('pb-pr-only','proj-1', NULL,      12, 'merged', datetime('now', '-8 days'), datetime('now', '-8 days'));

         INSERT INTO agent_conversation_workspaces (
             conversation_id, project_id, linked_plan_branch_id, publication_pr_number,
             publication_pr_status, created_at, updated_at
         ) VALUES
             ('direct-workspace', 'proj-1', NULL,   11, 'open',   datetime('now', '-9 days'), datetime('now', '-8 days')),
             ('execution-owned',  'proj-1', 'pb-1', 10, 'merged', datetime('now', '-9 days'), datetime('now', '-8 days'));

         INSERT INTO agent_conversation_workspace_publication_events (
             id, conversation_id, step, status, created_at
         ) VALUES ('direct-published', 'direct-workspace', 'published', 'succeeded', datetime('now', '-8 days'));",
    )
    .expect("insert test data");

    let trends = compute_project_trends(&conn, "proj-1", 0, 0).unwrap();
    let active = trends
        .weekly_delivery_throughput
        .iter()
        .find(|point| point.unified_deliveries > 0 || point.merged_prs > 0)
        .expect("delivery throughput point");

    assert_eq!(active.task_deliveries, 2);
    assert_eq!(active.workspace_deliveries, 1);
    assert_eq!(active.unified_deliveries, 3);
    assert_eq!(active.merged_prs, 2);
}
