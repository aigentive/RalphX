//! Integration tests: schema validation for metrics queries
//!
//! Verifies that all tables and columns required for the engineering metrics
//! feature exist after running migrations on a fresh in-memory database.
//!
//! ## Column name mapping (plan assumptions → actual schema)
//!
//! | Plan assumed | Actual column | Table |
//! |---|---|---|
//! | `tasks.status` | `tasks.internal_status` | tasks |
//! | `task_state_history.from_state` | `task_state_history.from_status` | task_state_history |
//! | `task_state_history.to_state` | `task_state_history.to_status` | task_state_history |
//! | `reviews.outcome` | `reviews.status` | reviews |

use ralphx_lib::testing::SqliteTestDb;

struct MetricsSchemaTestDb {
    _db: SqliteTestDb,
    conn: rusqlite::Connection,
}

impl std::ops::Deref for MetricsSchemaTestDb {
    type Target = rusqlite::Connection;

    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}

fn column_exists(conn: &rusqlite::Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info({})", table);
    let mut stmt = conn.prepare(&sql).expect("failed to prepare PRAGMA");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("failed to query table_info");
    let cols: Vec<String> = rows.flatten().collect();
    cols.iter().any(|col| col == column)
}

fn table_exists(conn: &rusqlite::Connection, table: &str) -> bool {
    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |row| row.get(0),
        )
        .unwrap_or(0);
    count > 0
}

fn index_exists(conn: &rusqlite::Connection, index: &str) -> bool {
    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
            [index],
            |row| row.get(0),
        )
        .unwrap_or(0);
    count > 0
}

fn setup_migrated_db() -> MetricsSchemaTestDb {
    let db = SqliteTestDb::new("metrics-schema-validation");
    let conn = db.new_connection();
    MetricsSchemaTestDb { _db: db, conn }
}

// ---------------------------------------------------------------------------
// Table existence
// ---------------------------------------------------------------------------

#[test]
fn test_tasks_table_exists() {
    let conn = setup_migrated_db();
    assert!(table_exists(&conn, "tasks"), "tasks table must exist");
}

#[test]
fn test_task_state_history_table_exists() {
    let conn = setup_migrated_db();
    assert!(
        table_exists(&conn, "task_state_history"),
        "task_state_history table must exist"
    );
}

#[test]
fn test_task_steps_table_exists() {
    let conn = setup_migrated_db();
    assert!(table_exists(&conn, "task_steps"), "task_steps table must exist");
}

#[test]
fn test_reviews_table_exists() {
    let conn = setup_migrated_db();
    assert!(table_exists(&conn, "reviews"), "reviews table must exist");
}

// ---------------------------------------------------------------------------
// tasks — required columns
// ---------------------------------------------------------------------------

#[test]
fn test_tasks_has_id() {
    let conn = setup_migrated_db();
    assert!(column_exists(&conn, "tasks", "id"));
}

#[test]
fn test_tasks_has_project_id() {
    let conn = setup_migrated_db();
    assert!(column_exists(&conn, "tasks", "project_id"));
}

/// The plan document referred to this as "status" but the actual column is "internal_status".
#[test]
fn test_tasks_has_internal_status_not_status() {
    let conn = setup_migrated_db();
    assert!(
        column_exists(&conn, "tasks", "internal_status"),
        "tasks.internal_status is the actual status column (plan assumed 'status')"
    );
}

#[test]
fn test_tasks_has_created_at() {
    let conn = setup_migrated_db();
    assert!(column_exists(&conn, "tasks", "created_at"));
}

#[test]
fn test_tasks_has_updated_at() {
    let conn = setup_migrated_db();
    assert!(column_exists(&conn, "tasks", "updated_at"));
}

// ---------------------------------------------------------------------------
// task_state_history — required columns
// ---------------------------------------------------------------------------

#[test]
fn test_task_state_history_has_task_id() {
    let conn = setup_migrated_db();
    assert!(column_exists(&conn, "task_state_history", "task_id"));
}

/// The plan document referred to this as "from_state" but the actual column is "from_status".
#[test]
fn test_task_state_history_has_from_status_not_from_state() {
    let conn = setup_migrated_db();
    assert!(
        column_exists(&conn, "task_state_history", "from_status"),
        "task_state_history.from_status is the actual column (plan assumed 'from_state')"
    );
}

/// The plan document referred to this as "to_state" but the actual column is "to_status".
#[test]
fn test_task_state_history_has_to_status_not_to_state() {
    let conn = setup_migrated_db();
    assert!(
        column_exists(&conn, "task_state_history", "to_status"),
        "task_state_history.to_status is the actual column (plan assumed 'to_state')"
    );
}

#[test]
fn test_task_state_history_has_created_at() {
    let conn = setup_migrated_db();
    assert!(column_exists(&conn, "task_state_history", "created_at"));
}

// ---------------------------------------------------------------------------
// task_steps — required columns
// ---------------------------------------------------------------------------

#[test]
fn test_task_steps_has_task_id() {
    let conn = setup_migrated_db();
    assert!(column_exists(&conn, "task_steps", "task_id"));
}

#[test]
fn test_task_steps_has_id() {
    let conn = setup_migrated_db();
    assert!(column_exists(&conn, "task_steps", "id"));
}

#[test]
fn test_task_steps_has_status() {
    let conn = setup_migrated_db();
    assert!(column_exists(&conn, "task_steps", "status"));
}

// ---------------------------------------------------------------------------
// reviews — required columns
// ---------------------------------------------------------------------------

#[test]
fn test_reviews_has_task_id() {
    let conn = setup_migrated_db();
    assert!(column_exists(&conn, "reviews", "task_id"));
}

/// The plan document referred to this as "outcome" but the actual column is "status".
#[test]
fn test_reviews_has_status_not_outcome() {
    let conn = setup_migrated_db();
    assert!(
        column_exists(&conn, "reviews", "status"),
        "reviews.status is the actual approval field (plan assumed 'outcome')"
    );
}

// ---------------------------------------------------------------------------
// Index presence
// ---------------------------------------------------------------------------

#[test]
fn test_metrics_composite_index_exists() {
    let conn = setup_migrated_db();
    assert!(
        index_exists(&conn, "idx_task_state_history_task_created"),
        "idx_task_state_history_task_created must exist for cycle time query performance"
    );
}
