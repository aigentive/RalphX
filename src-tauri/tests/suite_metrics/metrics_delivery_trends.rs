use chrono::{Datelike, Duration, Utc};
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

    // Anchor fixture timestamps to `now` (rather than hardcoded calendar dates) so they
    // stay inside the query's rolling 77-day window, and place them all inside the same
    // Sunday-start week bucket (offsets 0-2) so the fixture can't split across a week
    // boundary regardless of which weekday the test runs on.
    let now = Utc::now();
    let week_start =
        now - Duration::days(now.weekday().num_days_from_sunday() as i64) - Duration::days(7);
    let day = |offset: i64, hour: &str| {
        (week_start + Duration::days(offset))
            .format(&format!("%Y-%m-%dT{hour}:00:00+00:00"))
            .to_string()
    };
    let ready_at = day(0, "09");
    let workspace_created_at = day(1, "09");
    let workspace_published_at = day(1, "10");
    let merged_at = day(2, "09");
    let pr_only_merged_at = day(2, "11");

    conn.execute(
        "INSERT INTO tasks (id, project_id, internal_status, created_at, updated_at)
         VALUES ('task-1', 'proj-1', 'merged', ?1, ?2)",
        rusqlite::params![ready_at, merged_at],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, created_at)
         VALUES ('hist-ready', 'task-1', 'ready', ?1),
                ('hist-merged', 'task-1', 'merged', ?2)",
        rusqlite::params![ready_at, merged_at],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_branches (
            id, project_id, merge_task_id, pr_number, pr_status, merged_at, last_polled_at
         )
         VALUES
            ('pb-1', 'proj-1', 'task-1', 10, 'merged', ?1, ?1),
            ('pb-pr-only', 'proj-1', NULL, 12, 'merged', ?2, ?2)",
        rusqlite::params![merged_at, pr_only_merged_at],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_conversation_workspaces (
            conversation_id, project_id, linked_plan_branch_id, publication_pr_number,
            publication_pr_status, created_at, updated_at
         )
         VALUES
            ('direct-workspace', 'proj-1', NULL, 11, 'open', ?1, ?2),
            ('execution-owned', 'proj-1', 'pb-1', 10, 'merged', ?1, ?3)",
        rusqlite::params![workspace_created_at, workspace_published_at, merged_at],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_conversation_workspace_publication_events (
            id, conversation_id, step, status, created_at
         )
         VALUES ('direct-published', 'direct-workspace', 'published', 'succeeded', ?1)",
        rusqlite::params![workspace_published_at],
    )
    .unwrap();

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
