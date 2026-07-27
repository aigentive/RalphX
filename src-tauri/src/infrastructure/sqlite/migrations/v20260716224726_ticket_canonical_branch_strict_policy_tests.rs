//! Tests for migration v20260716224726: ticket canonical branch strict policy

use rusqlite::Connection;

use super::{
    v20260621201947_ticket_canonical_branches,
    v20260716224726_ticket_canonical_branch_strict_policy,
};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

fn create_legacy_row(conn: &Connection) {
    conn.execute(
        "INSERT INTO ticket_canonical_branches (
            project_id, provider, issue_key, branch_name, base_branch, base_commit,
            origin_pushed, terminal, created_at, updated_at
         ) VALUES (
            'project-1', 'linear', 'WISE-24', 'ralphx/ticket/linear-wise-24',
            'main', 'abc123', 1, 1, '2026-07-01T00:00:00Z', '2026-07-02T00:00:00Z'
         )",
        [],
    )
    .unwrap();
}

#[test]
fn adds_strict_policy_and_cycle_defaults_without_rewriting_legacy_rows() {
    let conn = setup_test_db();
    v20260621201947_ticket_canonical_branches::migrate(&conn).unwrap();
    create_legacy_row(&conn);

    v20260716224726_ticket_canonical_branch_strict_policy::migrate(&conn).unwrap();

    let stored = conn
        .query_row(
            "SELECT branch_name, terminal, policy_kind, policy_version,
                    task_title_snapshot, clickup_username_snapshot,
                    commit_subject_rule, pr_title_snapshot,
                    cycle_generation, cycle_state, cycle_base_commit,
                    cycle_effective_merge_base, cycle_started_at, cycle_terminal_at
               FROM ticket_canonical_branches
              WHERE project_id = 'project-1' AND provider = 'linear' AND issue_key = 'WISE-24'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(stored.0, "ralphx/ticket/linear-wise-24");
    assert!(stored.1, "the legacy terminal meaning is preserved");
    assert_eq!(stored.2, "legacy_canonical_base");
    assert_eq!(stored.3, None);
    assert_eq!(stored.4, None);
    assert_eq!(stored.5, None);
    assert_eq!(stored.6, None);
    assert_eq!(stored.7, None);
    assert_eq!(stored.8, 0);
    assert_eq!(stored.9, "legacy");
    assert_eq!(stored.10, None);
    assert_eq!(stored.11, None);
    assert_eq!(stored.12, None);
    assert_eq!(stored.13, None);
}

#[test]
fn enforces_one_branch_name_per_project_without_cross_project_collisions() {
    let conn = setup_test_db();
    v20260621201947_ticket_canonical_branches::migrate(&conn).unwrap();
    v20260716224726_ticket_canonical_branch_strict_policy::migrate(&conn).unwrap();
    create_legacy_row(&conn);

    let same_project = conn.execute(
        "INSERT INTO ticket_canonical_branches (
            project_id, provider, issue_key, branch_name, base_branch,
            created_at, updated_at
         ) VALUES (
            'project-1', 'clickup', 'CU-24', 'ralphx/ticket/linear-wise-24', 'main',
            '2026-07-03T00:00:00Z', '2026-07-03T00:00:00Z'
         )",
        [],
    );
    assert!(same_project.is_err());

    conn.execute(
        "INSERT INTO ticket_canonical_branches (
            project_id, provider, issue_key, branch_name, base_branch,
            created_at, updated_at
         ) VALUES (
            'project-2', 'clickup', 'CU-24', 'ralphx/ticket/linear-wise-24', 'main',
            '2026-07-03T00:00:00Z', '2026-07-03T00:00:00Z'
         )",
        [],
    )
    .unwrap();
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();
    v20260621201947_ticket_canonical_branches::migrate(&conn).unwrap();

    v20260716224726_ticket_canonical_branch_strict_policy::migrate(&conn).unwrap();
    v20260716224726_ticket_canonical_branch_strict_policy::migrate(&conn).unwrap();

    let strict_columns = conn
        .prepare("PRAGMA table_info(ticket_canonical_branches)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .into_iter()
        .filter(|column| {
            matches!(
                column.as_str(),
                "policy_kind"
                    | "policy_version"
                    | "task_title_snapshot"
                    | "clickup_username_snapshot"
                    | "commit_subject_rule"
                    | "pr_title_snapshot"
                    | "cycle_generation"
                    | "cycle_state"
                    | "cycle_base_commit"
                    | "cycle_effective_merge_base"
                    | "cycle_started_at"
                    | "cycle_terminal_at"
            )
        })
        .count();

    assert_eq!(strict_columns, 12);
}
