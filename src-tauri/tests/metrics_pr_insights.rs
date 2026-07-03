use rusqlite::Connection;

use ralphx_lib::commands::metrics_commands::{
    compute_insights_pr_insights, compute_project_pr_insights,
};

fn create_schema(conn: &Connection) {
    conn.execute_batch(
        "
        CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );

        CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            internal_status TEXT NOT NULL DEFAULT 'backlog',
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE task_state_history (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            from_status TEXT,
            to_status TEXT NOT NULL,
            changed_by TEXT NOT NULL DEFAULT 'system',
            created_at TEXT NOT NULL
        );

        CREATE TABLE plan_branches (
            id TEXT PRIMARY KEY,
            plan_artifact_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            branch_name TEXT NOT NULL,
            source_branch TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            merge_task_id TEXT NULL,
            created_at TEXT NOT NULL,
            merged_at TEXT NULL,
            execution_plan_id TEXT NULL,
            pr_number INTEGER NULL,
            pr_url TEXT NULL,
            pr_status TEXT NULL,
            pr_polling_active BOOLEAN NOT NULL DEFAULT 0,
            pr_eligible BOOLEAN NOT NULL DEFAULT 0,
            last_polled_at TEXT NULL,
            pr_push_status TEXT NOT NULL DEFAULT 'pending',
            merge_commit_sha TEXT NULL,
            pr_draft BOOLEAN NULL,
            base_branch_override TEXT NULL
        );

        CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            mode TEXT NOT NULL,
            base_ref_kind TEXT NOT NULL,
            base_ref TEXT NOT NULL,
            base_display_name TEXT NULL,
            base_commit TEXT NULL,
            branch_name TEXT NOT NULL,
            worktree_path TEXT NOT NULL,
            linked_ideation_session_id TEXT NULL,
            linked_plan_branch_id TEXT NULL,
            publication_pr_number INTEGER NULL,
            publication_pr_url TEXT NULL,
            publication_pr_status TEXT NULL,
            publication_push_status TEXT NULL,
            pr_autofix_enabled BOOLEAN NOT NULL DEFAULT 0,
            pr_auto_merge_desired BOOLEAN NOT NULL DEFAULT 0,
            pr_auto_merge_method TEXT NOT NULL DEFAULT 'squash',
            pr_auto_merge_current BOOLEAN NULL,
            pr_supervision_status TEXT NULL,
            pr_supervision_summary TEXT NULL,
            pr_supervision_updated_at TEXT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE agent_conversation_workspace_publication_events (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            step TEXT NOT NULL,
            status TEXT NOT NULL,
            summary TEXT NOT NULL,
            classification TEXT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE agent_conversation_workspace_state_history (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            state_family TEXT NOT NULL,
            from_state TEXT NULL,
            to_state TEXT NOT NULL,
            source TEXT NOT NULL,
            source_event_id TEXT NULL,
            created_at TEXT NOT NULL
        );
        ",
    )
    .expect("create schema");
}

fn insert_project(conn: &Connection, project_id: &str) {
    conn.execute(
        "INSERT INTO projects (id, name) VALUES (?1, 'Project')",
        rusqlite::params![project_id],
    )
    .unwrap();
}

fn insert_merge_task(conn: &Connection, task_id: &str, project_id: &str) {
    conn.execute(
        "INSERT INTO tasks (id, project_id, internal_status, created_at, updated_at)
         VALUES (?1, ?2, 'merged', '2026-05-01T09:00:00+00:00', '2026-05-05T12:00:00+00:00')",
        rusqlite::params![task_id, project_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, created_at)
         VALUES ('h-wait', ?1, 'waiting_on_pr', '2026-05-04T12:00:00+00:00')",
        rusqlite::params![task_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, created_at)
         VALUES ('h-merged', ?1, 'merged', '2026-05-05T12:00:00+00:00')",
        rusqlite::params![task_id],
    )
    .unwrap();
}

fn insert_plan_pr(conn: &Connection, project_id: &str, task_id: &str) {
    conn.execute(
        "INSERT INTO plan_branches (
            id, plan_artifact_id, session_id, project_id, branch_name, source_branch, status,
            merge_task_id, created_at, merged_at, pr_number, pr_url, pr_status, pr_eligible,
            pr_push_status, pr_draft
         )
         VALUES (
            'pb-1', 'artifact-1', 'session-1', ?1, 'plan/pr-branch', 'main', 'merged',
            ?2, '2026-05-01T10:00:00+00:00', '2026-05-05T12:00:00+00:00',
            10, 'https://github.test/org/repo/pull/10', 'Merged', 1, 'pushed', 0
         )",
        rusqlite::params![project_id, task_id],
    )
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn insert_workspace(
    conn: &Connection,
    conversation_id: &str,
    project_id: &str,
    linked_plan_branch_id: Option<&str>,
    pr_number: Option<i64>,
    pr_status: Option<&str>,
    push_status: Option<&str>,
    supervision_status: Option<&str>,
) {
    conn.execute(
        "INSERT INTO agent_conversation_workspaces (
            conversation_id, project_id, mode, base_ref_kind, base_ref, branch_name, worktree_path,
            linked_plan_branch_id, publication_pr_number, publication_pr_url,
            publication_pr_status, publication_push_status, pr_autofix_enabled,
            pr_auto_merge_desired, pr_auto_merge_current, pr_supervision_status,
            status, created_at, updated_at
         )
         VALUES (
            ?1, ?2, 'edit', 'project_default', 'main', ?3, '/tmp/worktree',
            ?4, ?5, ?6, ?7, ?8, 1, 1, 1, ?9, 'active',
            '2026-05-02T09:00:00+00:00', '2026-05-03T09:00:00+00:00'
         )",
        rusqlite::params![
            conversation_id,
            project_id,
            format!("rx/{conversation_id}"),
            linked_plan_branch_id,
            pr_number,
            pr_number.map(|n| format!("https://github.test/org/repo/pull/{n}")),
            pr_status,
            push_status,
            supervision_status
        ],
    )
    .unwrap();
}

fn insert_event(
    conn: &Connection,
    id: &str,
    conversation_id: &str,
    step: &str,
    status: &str,
    created_at: &str,
) {
    conn.execute(
        "INSERT INTO agent_conversation_workspace_publication_events
            (id, conversation_id, step, status, summary, created_at)
         VALUES (?1, ?2, ?3, ?4, 'event', ?5)",
        rusqlite::params![id, conversation_id, step, status, created_at],
    )
    .unwrap();
}

#[test]
fn pr_insights_dedupes_execution_owned_workspace_from_plan_pr_totals() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj-1");
    insert_merge_task(&conn, "merge-task-1", "proj-1");
    insert_plan_pr(&conn, "proj-1", "merge-task-1");

    insert_workspace(
        &conn,
        "direct-merged",
        "proj-1",
        None,
        Some(11),
        Some("merged"),
        Some("pushed"),
        Some("monitoring"),
    );
    insert_event(
        &conn,
        "direct-pushed",
        "direct-merged",
        "pushed",
        "succeeded",
        "2026-05-02T09:00:00+00:00",
    );
    insert_event(
        &conn,
        "direct-merged-event",
        "direct-merged",
        "pr_merged",
        "succeeded",
        "2026-05-03T09:00:00+00:00",
    );
    conn.execute(
        "INSERT INTO agent_conversation_workspace_state_history (
            id, conversation_id, state_family, from_state, to_state, source, source_event_id, created_at
         )
         VALUES
            ('hist-push', 'direct-merged', 'publication_push_status', NULL, 'pushed', 'publication_event_backfill', 'direct-pushed', '2026-05-02T09:00:00+00:00'),
            ('hist-merged', 'direct-merged', 'publication_push_status', 'pushed', 'refreshed', 'publication_event_backfill', 'direct-merged-event', '2026-05-03T09:00:00+00:00')",
        [],
    )
    .unwrap();

    insert_workspace(
        &conn,
        "direct-review",
        "proj-1",
        None,
        Some(12),
        Some("changes_requested"),
        Some("needs_agent"),
        Some("fixing"),
    );
    insert_event(
        &conn,
        "review-feedback",
        "direct-review",
        "github_review",
        "needs_agent",
        "2026-05-04T09:00:00+00:00",
    );
    insert_event(
        &conn,
        "autofix-needed",
        "direct-review",
        "pr_autofix",
        "needs_agent",
        "2026-05-04T10:00:00+00:00",
    );

    insert_workspace(
        &conn,
        "execution-owned",
        "proj-1",
        Some("pb-1"),
        Some(10),
        Some("merged"),
        Some("pushed"),
        Some("monitoring"),
    );
    insert_workspace(&conn, "without-pr", "proj-1", None, None, None, None, None);

    let insights = compute_project_pr_insights(&conn, "proj-1", 0, 0).unwrap();

    assert_eq!(insights.summary.total_prs, 3);
    assert_eq!(insights.summary.direct_workspace_prs, 2);
    assert_eq!(insights.summary.task_pipeline_prs, 1);
    assert_eq!(insights.summary.execution_owned_workspace_refs, 1);
    assert_eq!(insights.summary.merged_prs, 2);
    assert_eq!(insights.summary.changes_requested_prs, 1);
    assert_eq!(insights.summary.needs_agent_prs, 1);
    assert_eq!(insights.summary.total_workspaces, 4);
    assert_eq!(insights.summary.direct_workspaces, 3);
    assert_eq!(insights.summary.direct_workspaces_with_prs, 2);
    assert!((insights.summary.direct_workspace_pr_conversion_rate - (2.0 / 3.0)).abs() < 1e-9);
    assert!((insights.summary.terminal_merge_rate - 1.0).abs() < 1e-9);
    assert_eq!(insights.summary.requested_changes_events, 1);
    assert_eq!(insights.summary.autofix_needed_events, 1);
    assert_eq!(insights.summary.supervision_enabled_workspaces, 3);
    assert_eq!(insights.summary.auto_merge_active_workspaces, 3);
    assert_eq!(insights.workspace_dwell_times.len(), 1);
    assert_eq!(
        insights.workspace_dwell_times[0].label,
        "Publication: Pushed"
    );
    assert!((insights.workspace_dwell_times[0].avg_minutes - 1440.0).abs() < 1e-9);

    let plan_origin = insights
        .origins
        .iter()
        .find(|origin| origin.origin == "task_pipeline_pr_mode")
        .expect("plan origin");
    assert!(plan_origin.counted_in_totals);
    assert_eq!(plan_origin.total_prs, 1);

    let execution_origin = insights
        .origins
        .iter()
        .find(|origin| origin.origin == "agent_workspace_execution_owned")
        .expect("execution-owned origin");
    assert!(!execution_origin.counted_in_totals);
    assert_eq!(execution_origin.total_prs, 1);
}

#[test]
fn pr_insights_aggregate_direct_workspaces_across_projects() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj-1");
    insert_project(&conn, "proj-2");

    insert_workspace(
        &conn,
        "proj-1-direct",
        "proj-1",
        None,
        Some(11),
        Some("merged"),
        Some("pushed"),
        Some("monitoring"),
    );
    insert_workspace(
        &conn,
        "proj-2-direct",
        "proj-2",
        None,
        Some(21),
        Some("open"),
        Some("pushed"),
        None,
    );

    let project_insights = compute_project_pr_insights(&conn, "proj-1", 0, 0).unwrap();
    let aggregate_insights = compute_insights_pr_insights(&conn, 0, 0).unwrap();

    assert_eq!(project_insights.summary.total_prs, 1);
    assert_eq!(aggregate_insights.summary.total_prs, 2);
    assert_eq!(aggregate_insights.summary.direct_workspace_prs, 2);
    assert_eq!(aggregate_insights.summary.total_workspaces, 2);
    assert_eq!(aggregate_insights.summary.merged_prs, 1);
    assert_eq!(aggregate_insights.summary.open_prs, 1);
}
