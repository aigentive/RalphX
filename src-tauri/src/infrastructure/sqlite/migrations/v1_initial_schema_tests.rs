use super::*;
use crate::infrastructure::sqlite::connection::open_memory_connection;

// ==========================================================================
// Core migration system tests
// ==========================================================================

#[test]
fn test_schema_version_constant_matches_latest_registered_migration() {
    assert_eq!(SCHEMA_VERSION, latest_registered_migration_version());
}

#[test]
fn test_run_migrations_creates_migrations_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 1);
}

#[test]
fn test_run_migrations_sets_schema_version() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    let version = get_schema_version(&conn).unwrap();
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn test_run_migrations_is_idempotent() {
    let conn = open_memory_connection().unwrap();

    // Run migrations twice
    run_migrations(&conn).unwrap();
    run_migrations(&conn).unwrap();

    // Should still work and have correct version
    let version = get_schema_version(&conn).unwrap();
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn test_run_migrations_repairs_skipped_external_session_reliability_columns() {
    let conn = open_memory_connection().unwrap();
    create_migrations_table(&conn).unwrap();

    for migration in MIGRATIONS
        .iter()
        .filter(|migration| migration.version <= 78)
    {
        (migration.migrate)(&conn).unwrap();
        set_schema_version(&conn, migration.version).unwrap();
    }

    // Simulate databases that recorded the earlier meaning of v79 before the
    // external-session columns were later introduced at the same version.
    v80_dependencies_acknowledged::migrate(&conn).unwrap();
    set_schema_version(&conn, 79).unwrap();
    set_schema_version(&conn, 80).unwrap();

    assert!(!helpers::column_exists(
        &conn,
        "ideation_sessions",
        "api_key_id"
    ));
    assert!(!helpers::column_exists(
        &conn,
        "ideation_sessions",
        "idempotency_key"
    ));
    assert!(!helpers::column_exists(
        &conn,
        "ideation_sessions",
        "external_activity_phase"
    ));
    assert!(!helpers::column_exists(
        &conn,
        "ideation_sessions",
        "external_last_read_message_id"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "dependencies_acknowledged"
    ));

    run_migrations(&conn).unwrap();

    assert_eq!(get_schema_version(&conn).unwrap(), SCHEMA_VERSION);
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "api_key_id"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "idempotency_key"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "external_activity_phase"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "external_last_read_message_id"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_ideation_sessions_idempotency"
    ));
}

#[test]
fn test_run_migrations_applies_missing_registered_versions_below_current_max() {
    let conn = open_memory_connection().unwrap();
    create_migrations_table(&conn).unwrap();

    for migration in MIGRATIONS
        .iter()
        .filter(|migration| migration.version <= 20260521150003)
    {
        (migration.migrate)(&conn).unwrap();
        set_schema_version(&conn, migration.version).unwrap();
    }

    let later_migration = MIGRATIONS
        .iter()
        .find(|migration| migration.version == 20260527033000)
        .expect("later migration should exist");
    (later_migration.migrate)(&conn).unwrap();
    set_schema_version(&conn, later_migration.version).unwrap();

    assert_eq!(get_schema_version(&conn).unwrap(), 20260527033000);
    assert!(!helpers::column_exists(
        &conn,
        "ideation_sessions",
        "session_flow"
    ));
    assert!(!helpers::table_exists(&conn, "plan_artifact_approvals"));

    run_migrations(&conn).unwrap();

    assert_eq!(get_schema_version(&conn).unwrap(), SCHEMA_VERSION);
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "session_flow"
    ));
    assert!(helpers::table_exists(&conn, "plan_artifact_approvals"));
    assert!(helpers::table_exists(&conn, "plan_complexity_assessments"));

    let chat_conversations_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'chat_conversations'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(chat_conversations_sql.contains("'plan'"));
}

// ==========================================================================
// Core tables existence tests
// ==========================================================================

#[test]
fn test_creates_projects_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    let result = conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    );
    assert!(result.is_ok());
}

#[test]
fn test_creates_tasks_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO tasks (id, project_id, category, title) VALUES ('t1', 'p1', 'feature', 'Task')",
        [],
    );
    assert!(result.is_ok());
}

#[test]
fn test_tasks_has_all_columns() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    )
    .unwrap();

    // Test that all expected columns exist
    let result = conn.execute(
        "INSERT INTO tasks (
            id, project_id, category, title, description, priority,
            internal_status, needs_qa, qa_prep_status, qa_test_status,
            needs_review_point, external_status, wave, checkpoint_type,
            phase_id, plan_id, must_haves_json, archived_at
        ) VALUES (
            't1', 'p1', 'feature', 'Task', 'desc', 1,
            'backlog', 1, 'pending', 'pending',
            0, NULL, NULL, NULL,
            NULL, NULL, NULL, NULL
        )",
        [],
    );
    assert!(result.is_ok());
}

// ==========================================================================
// Relationship tables tests
// ==========================================================================

#[test]
fn test_creates_task_dependencies_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title) VALUES ('t1', 'p1', 'feature', 'Task 1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title) VALUES ('t2', 'p1', 'feature', 'Task 2')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO task_dependencies (id, task_id, depends_on_task_id) VALUES ('d1', 't1', 't2')",
        [],
    );
    assert!(result.is_ok());
}

#[test]
fn test_task_dependencies_self_reference_check() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title) VALUES ('t1', 'p1', 'feature', 'Task')",
        [],
    )
    .unwrap();

    // Self-reference should be rejected by CHECK constraint
    let result = conn.execute(
        "INSERT INTO task_dependencies (id, task_id, depends_on_task_id) VALUES ('d1', 't1', 't1')",
        [],
    );
    assert!(result.is_err());
}

#[test]
fn test_task_dependencies_unique_constraint() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title) VALUES ('t1', 'p1', 'feature', 'Task 1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title) VALUES ('t2', 'p1', 'feature', 'Task 2')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO task_dependencies (id, task_id, depends_on_task_id) VALUES ('d1', 't1', 't2')",
        [],
    )
    .unwrap();

    // Duplicate should be rejected by UNIQUE constraint
    let result = conn.execute(
        "INSERT INTO task_dependencies (id, task_id, depends_on_task_id) VALUES ('d2', 't1', 't2')",
        [],
    );
    assert!(result.is_err());
}

#[test]
fn test_task_blockers_table_dropped_by_v45() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    // The task_blockers table was dropped by migration v45.
    // Verify the table no longer exists.
    let result = conn.execute(
        "INSERT INTO task_blockers (task_id, blocker_id) VALUES ('t1', 't2')",
        [],
    );
    assert!(result.is_err());
}

// ==========================================================================
// State tracking tables tests
// ==========================================================================

#[test]
fn test_creates_task_state_history_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title) VALUES ('t1', 'p1', 'feature', 'Task')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO task_state_history (id, task_id, from_status, to_status, changed_by)
         VALUES ('h1', 't1', 'backlog', 'ready', 'user')",
        [],
    );
    assert!(result.is_ok());
}

#[test]
fn test_creates_task_state_data_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title) VALUES ('t1', 'p1', 'feature', 'Task')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO task_state_data (task_id, state_type, data) VALUES ('t1', 'executing', '{}')",
        [],
    );
    assert!(result.is_ok());
}

// ==========================================================================
// Review system tables tests
// ==========================================================================

#[test]
fn test_creates_reviews_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title) VALUES ('t1', 'p1', 'feature', 'Task')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO reviews (id, project_id, task_id, reviewer_type) VALUES ('r1', 'p1', 't1', 'ai')",
        [],
    );
    assert!(result.is_ok());
}

#[test]
fn test_creates_review_actions_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title) VALUES ('t1', 'p1', 'feature', 'Task')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO reviews (id, project_id, task_id, reviewer_type) VALUES ('r1', 'p1', 't1', 'ai')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO review_actions (id, review_id, action_type) VALUES ('a1', 'r1', 'approved')",
        [],
    );
    assert!(result.is_ok());
}

#[test]
fn test_creates_review_notes_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title) VALUES ('t1', 'p1', 'feature', 'Task')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO review_notes (id, task_id, reviewer, outcome) VALUES ('n1', 't1', 'ai', 'approved')",
        [],
    );
    assert!(result.is_ok());
}

// ==========================================================================
// Ideation system tables tests
// ==========================================================================

#[test]
fn test_creates_ideation_sessions_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, title) VALUES ('s1', 'p1', 'Session')",
        [],
    );
    assert!(result.is_ok());
}

#[test]
fn test_creates_task_proposals_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'Test', '/path')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, title) VALUES ('s1', 'p1', 'Session')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO task_proposals (id, session_id, title, category, suggested_priority)
         VALUES ('tp1', 's1', 'Proposal', 'feature', 'medium')",
        [],
    );
    assert!(result.is_ok());
}

// ==========================================================================
// Chat system tables tests
// ==========================================================================

#[test]
fn test_creates_chat_conversations_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    let result = conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, created_at, updated_at)
         VALUES ('c1', 'ideation', 's1', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
        [],
    );
    assert!(result.is_ok());
}

#[test]
fn test_creates_chat_messages_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    let result = conn.execute(
        "INSERT INTO chat_messages (id, role, content) VALUES ('m1', 'user', 'Hello')",
        [],
    );
    assert!(result.is_ok());
}

// ==========================================================================
// Artifact system tables tests
// ==========================================================================

#[test]
fn test_creates_artifacts_table() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    let result = conn.execute(
        "INSERT INTO artifacts (id, type, name, content_type, created_by)
         VALUES ('a1', 'code', 'file.rs', 'text/plain', 'user')",
        [],
    );
    assert!(result.is_ok());
}

// ==========================================================================
// Settings tables tests
// ==========================================================================

#[test]
fn test_review_settings_has_defaults() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM review_settings", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);

    let require_workspace_review: i64 = conn
        .query_row(
            "SELECT require_workspace_review FROM review_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(require_workspace_review, 1);

    let autofix_workspace_review_blocking_findings: i64 = conn
        .query_row(
            "SELECT autofix_workspace_review_blocking_findings FROM review_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(autofix_workspace_review_blocking_findings, 1);
}

#[test]
fn test_ideation_settings_has_defaults() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM ideation_settings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}
