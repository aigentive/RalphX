//! Tests for migration v51: repair plan_branches schema + backfill execution_plans
//!
//! v51 handles TWO scenarios:
//! - **Fresh DB**: v46-v50 all ran → plan_branches has execution_plan_id + UNIQUE on plan_artifact_id
//! - **Dev DB**: old v49 dropped execution_plans → plan_branches has 10 cols, no execution_plan_id

use rusqlite::Connection;

use super::helpers;
use super::v51_repair_plan_branches;

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

/// Set up a "fresh DB" scenario: v46-v50 all ran correctly.
/// plan_branches has 11 columns including execution_plan_id + UNIQUE on plan_artifact_id.
fn setup_fresh_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    conn.execute_batch(
        "PRAGMA foreign_keys = OFF;

        CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );

        CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            title TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00+00:00',
            updated_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00+00:00',
            converted_at TEXT
        );

        CREATE TABLE execution_plans (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES ideation_sessions(id),
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );

        -- Fresh DB: plan_branches has 11 columns, UNIQUE on plan_artifact_id (from v13)
        CREATE TABLE plan_branches (
            id TEXT PRIMARY KEY,
            plan_artifact_id TEXT NOT NULL UNIQUE,
            session_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            branch_name TEXT NOT NULL,
            source_branch TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            merge_task_id TEXT,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            merged_at TEXT,
            execution_plan_id TEXT REFERENCES execution_plans(id)
        );

        CREATE UNIQUE INDEX idx_plan_branches_session_id
            ON plan_branches(session_id);

        CREATE UNIQUE INDEX idx_plan_branches_execution_plan
            ON plan_branches(execution_plan_id) WHERE execution_plan_id IS NOT NULL;

        CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            title TEXT NOT NULL,
            ideation_session_id TEXT,
            execution_plan_id TEXT
        );

        CREATE TABLE project_active_plan (
            project_id TEXT PRIMARY KEY,
            ideation_session_id TEXT NOT NULL,
            execution_plan_id TEXT,
            updated_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00+00:00'
        );",
    )
    .expect("Failed to create fresh DB schema");

    conn
}

/// Set up a "dev DB" scenario: old v49_fix_ghost_plan_branches ran,
/// dropping execution_plans table and removing execution_plan_id from plan_branches.
/// plan_branches has 10 columns, NO execution_plan_id.
fn setup_dev_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    conn.execute_batch(
        "PRAGMA foreign_keys = OFF;

        CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );

        CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            title TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00+00:00',
            updated_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00+00:00',
            converted_at TEXT
        );

        -- Dev DB: execution_plans table was DROPPED by old v49
        -- (v51 will recreate it)

        -- Dev DB: plan_branches has 10 columns, NO execution_plan_id, NO UNIQUE on plan_artifact_id
        CREATE TABLE plan_branches (
            id TEXT PRIMARY KEY,
            plan_artifact_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            branch_name TEXT NOT NULL,
            source_branch TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            merge_task_id TEXT,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            merged_at TEXT
        );

        CREATE UNIQUE INDEX idx_plan_branches_session_id
            ON plan_branches(session_id);

        CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            title TEXT NOT NULL,
            ideation_session_id TEXT,
            execution_plan_id TEXT
        );

        CREATE TABLE project_active_plan (
            project_id TEXT PRIMARY KEY,
            ideation_session_id TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00+00:00'
        );",
    )
    .expect("Failed to create dev DB schema");

    conn
}

/// Helper to check if a column exists in the plan_branches table
fn plan_branches_has_column(conn: &Connection, column: &str) -> bool {
    helpers::column_exists(conn, "plan_branches", column)
}

/// Helper to check if a UNIQUE constraint exists on a column
fn has_unique_index(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND tbl_name='{}' AND sql LIKE '%UNIQUE%' AND sql LIKE '%{}%'",
        table, column
    );
    let count: i32 = conn.query_row(&sql, [], |row| row.get(0)).unwrap_or(0);
    count > 0
}

/// Helper to check if plan_artifact_id has a UNIQUE constraint
fn plan_artifact_id_is_unique(conn: &Connection) -> bool {
    // Check both inline UNIQUE and UNIQUE INDEX
    let inline: bool = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='plan_branches'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|sql| sql.contains("UNIQUE") && sql.contains("plan_artifact_id"))
        .unwrap_or(false);

    let index = has_unique_index(conn, "plan_branches", "plan_artifact_id");

    inline || index
}

// ---------------------------------------------------------------------------
// Fresh DB path tests
// ---------------------------------------------------------------------------

#[test]
fn test_fresh_db_drops_unique_on_plan_artifact_id() {
    let conn = setup_fresh_db();

    // Before: plan_artifact_id has UNIQUE constraint
    assert!(
        plan_artifact_id_is_unique(&conn),
        "Fresh DB should start with UNIQUE on plan_artifact_id"
    );

    v51_repair_plan_branches::migrate(&conn).unwrap();

    // After: UNIQUE is removed (multiple sessions can share same plan_artifact_id)
    assert!(
        !plan_artifact_id_is_unique(&conn),
        "v51 should remove UNIQUE on plan_artifact_id"
    );
}

#[test]
fn test_fresh_db_preserves_execution_plan_id_column() {
    let conn = setup_fresh_db();

    assert!(plan_branches_has_column(&conn, "execution_plan_id"));

    v51_repair_plan_branches::migrate(&conn).unwrap();

    assert!(
        plan_branches_has_column(&conn, "execution_plan_id"),
        "v51 should preserve execution_plan_id column on fresh DB"
    );
}

#[test]
fn test_fresh_db_preserves_plan_branch_data() {
    let conn = setup_fresh_db();

    // Insert test data
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO execution_plans (id, session_id, status) VALUES ('ep1', 's1', 'active')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch, status, execution_plan_id)
         VALUES ('pb1', 'art1', 's1', 'p1', 'ralphx/test/plan-abc', 'main', 'active', 'ep1')",
        [],
    )
    .unwrap();

    v51_repair_plan_branches::migrate(&conn).unwrap();

    // Verify data preserved
    let (id, artifact, session, project, branch, source, status, ep): (String, String, String, String, String, String, String, Option<String>) = conn
        .query_row(
            "SELECT id, plan_artifact_id, session_id, project_id, branch_name, source_branch, status, execution_plan_id FROM plan_branches WHERE id = 'pb1'",
            [],
            |row| Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            )),
        )
        .unwrap();

    assert_eq!(id, "pb1");
    assert_eq!(artifact, "art1");
    assert_eq!(session, "s1");
    assert_eq!(project, "p1");
    assert_eq!(branch, "ralphx/test/plan-abc");
    assert_eq!(source, "main");
    assert_eq!(status, "active");
    assert_eq!(ep, Some("ep1".to_string()));
}

#[test]
fn test_fresh_db_backfill_is_noop_when_v49_already_ran() {
    let conn = setup_fresh_db();

    // Session already has an execution_plan (from v49 backfill)
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO execution_plans (id, session_id, status) VALUES ('ep-from-v49', 's1', 'active')",
        [],
    )
    .unwrap();

    v51_repair_plan_branches::migrate(&conn).unwrap();

    // Should still have exactly 1 execution_plan (no duplicate created)
    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM execution_plans", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        count, 1,
        "v51 should not create duplicate execution_plans when v49 already ran"
    );
}

// ---------------------------------------------------------------------------
// Dev DB path tests
// ---------------------------------------------------------------------------

#[test]
fn test_dev_db_recreates_execution_plans_table() {
    let conn = setup_dev_db();

    // Before: no execution_plans table
    assert!(
        !helpers::table_exists(&conn, "execution_plans"),
        "Dev DB should not have execution_plans table"
    );

    v51_repair_plan_branches::migrate(&conn).unwrap();

    // After: execution_plans table exists
    assert!(
        helpers::table_exists(&conn, "execution_plans"),
        "v51 should recreate execution_plans on dev DB"
    );
}

#[test]
fn test_dev_db_adds_execution_plan_id_to_plan_branches() {
    let conn = setup_dev_db();

    // Before: no execution_plan_id column
    assert!(
        !plan_branches_has_column(&conn, "execution_plan_id"),
        "Dev DB plan_branches should not have execution_plan_id column"
    );

    v51_repair_plan_branches::migrate(&conn).unwrap();

    // After: execution_plan_id column exists
    assert!(
        plan_branches_has_column(&conn, "execution_plan_id"),
        "v51 should add execution_plan_id to plan_branches on dev DB"
    );
}

#[test]
fn test_dev_db_preserves_plan_branch_data() {
    let conn = setup_dev_db();

    // Insert test data (10-column plan_branches)
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch, status)
         VALUES ('pb1', 'art1', 's1', 'p1', 'ralphx/test/plan-abc', 'main', 'merged')",
        [],
    )
    .unwrap();

    v51_repair_plan_branches::migrate(&conn).unwrap();

    // Verify data preserved through recreation
    let (id, artifact, session, project, branch, status): (String, String, String, String, String, String) = conn
        .query_row(
            "SELECT id, plan_artifact_id, session_id, project_id, branch_name, status FROM plan_branches WHERE id = 'pb1'",
            [],
            |row| Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            )),
        )
        .unwrap();

    assert_eq!(id, "pb1");
    assert_eq!(artifact, "art1");
    assert_eq!(session, "s1");
    assert_eq!(project, "p1");
    assert_eq!(branch, "ralphx/test/plan-abc");
    assert_eq!(status, "merged");

    // execution_plan_id should be populated by backfill (session is 'accepted')
    let ep: Option<String> = conn
        .query_row(
            "SELECT execution_plan_id FROM plan_branches WHERE id = 'pb1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        ep.is_some(),
        "execution_plan_id should be backfilled for accepted session branches on dev DB"
    );
}

#[test]
fn test_dev_db_backfill_creates_execution_plans() {
    let conn = setup_dev_db();

    // Accepted session with tasks and branches
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, ideation_session_id) VALUES ('t1', 'p1', 'Task 1', 's1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch)
         VALUES ('pb1', 'art1', 's1', 'p1', 'ralphx/test/plan-abc', 'main')",
        [],
    )
    .unwrap();

    v51_repair_plan_branches::migrate(&conn).unwrap();

    // Execution plan created
    let ep_count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM execution_plans WHERE session_id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        ep_count, 1,
        "v51 backfill should create execution_plan for accepted session on dev DB"
    );

    // Task linked
    let ep_id: String = conn
        .query_row(
            "SELECT id FROM execution_plans WHERE session_id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let task_ep: Option<String> = conn
        .query_row(
            "SELECT execution_plan_id FROM tasks WHERE id = 't1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        task_ep,
        Some(ep_id.clone()),
        "task should be linked to execution_plan"
    );

    // Plan branch linked
    let pb_ep: Option<String> = conn
        .query_row(
            "SELECT execution_plan_id FROM plan_branches WHERE id = 'pb1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        pb_ep,
        Some(ep_id),
        "plan_branch should be linked to execution_plan"
    );
}

#[test]
fn test_dev_db_backfill_links_active_plan() {
    let conn = setup_dev_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch)
         VALUES ('pb1', 'art1', 's1', 'p1', 'ralphx/test/plan', 'main')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO project_active_plan (project_id, ideation_session_id) VALUES ('p1', 's1')",
        [],
    )
    .unwrap();

    v51_repair_plan_branches::migrate(&conn).unwrap();

    let ep_id: String = conn
        .query_row(
            "SELECT id FROM execution_plans WHERE session_id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let active_ep: Option<String> = conn
        .query_row(
            "SELECT execution_plan_id FROM project_active_plan WHERE project_id = 'p1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        active_ep,
        Some(ep_id),
        "project_active_plan should be linked to execution_plan on dev DB"
    );
}

// ---------------------------------------------------------------------------
// Index verification tests
// ---------------------------------------------------------------------------

#[test]
fn test_indexes_created_after_migration() {
    let conn = setup_fresh_db();

    v51_repair_plan_branches::migrate(&conn).unwrap();

    // UNIQUE index on session_id
    assert!(
        helpers::index_exists(&conn, "idx_plan_branches_session_id"),
        "idx_plan_branches_session_id should exist"
    );

    // Non-unique index on plan_artifact_id
    assert!(
        helpers::index_exists(&conn, "idx_plan_branches_plan_artifact_id"),
        "idx_plan_branches_plan_artifact_id should exist"
    );

    // Unique index on execution_plan_id (where not null)
    assert!(
        helpers::index_exists(&conn, "idx_plan_branches_execution_plan"),
        "idx_plan_branches_execution_plan should exist"
    );
}

#[test]
fn test_dev_db_indexes_created_after_migration() {
    let conn = setup_dev_db();

    // Need a session for plan_branches FK
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'active')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch)
         VALUES ('pb1', 'art1', 's1', 'p1', 'ralphx/test/plan', 'main')",
        [],
    )
    .unwrap();

    v51_repair_plan_branches::migrate(&conn).unwrap();

    assert!(helpers::index_exists(&conn, "idx_plan_branches_session_id"));
    assert!(helpers::index_exists(
        &conn,
        "idx_plan_branches_plan_artifact_id"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_plan_branches_execution_plan"
    ));
}

// ---------------------------------------------------------------------------
// The key bug fix: multiple sessions can share plan_artifact_id
// ---------------------------------------------------------------------------

#[test]
fn test_multiple_sessions_share_plan_artifact_id_after_migration() {
    let conn = setup_fresh_db();

    // Pre-populate with data that would fail with UNIQUE constraint
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s2', 'p1', 'accepted')",
        [],
    )
    .unwrap();

    // Only one plan_branch can exist before migration (UNIQUE on plan_artifact_id)
    conn.execute(
        "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch)
         VALUES ('pb1', 'shared-artifact', 's1', 'p1', 'ralphx/test/plan-v1', 'main')",
        [],
    )
    .unwrap();

    v51_repair_plan_branches::migrate(&conn).unwrap();

    // After migration: inserting a second row with same plan_artifact_id should succeed
    let result = conn.execute(
        "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch)
         VALUES ('pb2', 'shared-artifact', 's2', 'p1', 'ralphx/test/plan-v2', 'main')",
        [],
    );

    assert!(
        result.is_ok(),
        "After v51, multiple plan_branches should be able to share the same plan_artifact_id. Got: {:?}",
        result.err()
    );

    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM plan_branches WHERE plan_artifact_id = 'shared-artifact'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 2,
        "Both plan_branches with shared artifact should exist"
    );
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

#[test]
fn test_migration_idempotent_on_fresh_db() {
    let conn = setup_fresh_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch)
         VALUES ('pb1', 'art1', 's1', 'p1', 'ralphx/test/plan', 'main')",
        [],
    )
    .unwrap();

    // Run twice
    v51_repair_plan_branches::migrate(&conn).unwrap();
    v51_repair_plan_branches::migrate(&conn).unwrap();

    // Data still intact
    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM plan_branches", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        count, 1,
        "plan_branches data should survive double migration"
    );

    let ep_count: i32 = conn
        .query_row("SELECT COUNT(*) FROM execution_plans", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        ep_count, 1,
        "should have exactly 1 execution_plan after double migration"
    );
}

// ---------------------------------------------------------------------------
// Tasks index verification
// ---------------------------------------------------------------------------

#[test]
fn test_tasks_execution_plan_index_created() {
    let conn = setup_fresh_db();

    v51_repair_plan_branches::migrate(&conn).unwrap();

    assert!(
        helpers::index_exists(&conn, "idx_tasks_execution_plan"),
        "idx_tasks_execution_plan should exist after v51"
    );
}

// ---------------------------------------------------------------------------
// Column additions are idempotent
// ---------------------------------------------------------------------------

#[test]
fn test_project_active_plan_execution_plan_id_added() {
    let conn = setup_dev_db();

    // Dev DB may not have execution_plan_id on project_active_plan
    assert!(
        !helpers::column_exists(&conn, "project_active_plan", "execution_plan_id"),
        "Dev DB project_active_plan should not have execution_plan_id yet"
    );

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'active')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch)
         VALUES ('pb1', 'art1', 's1', 'p1', 'ralphx/test/plan', 'main')",
        [],
    )
    .unwrap();

    v51_repair_plan_branches::migrate(&conn).unwrap();

    assert!(
        helpers::column_exists(&conn, "project_active_plan", "execution_plan_id"),
        "v51 should add execution_plan_id to project_active_plan"
    );
}

#[test]
fn test_tasks_execution_plan_id_column_idempotent() {
    let conn = setup_fresh_db();

    // Fresh DB already has execution_plan_id on tasks
    assert!(helpers::column_exists(&conn, "tasks", "execution_plan_id"));

    v51_repair_plan_branches::migrate(&conn).unwrap();

    // Column still exists (add_column_if_not_exists is idempotent)
    assert!(helpers::column_exists(&conn, "tasks", "execution_plan_id"));
}
