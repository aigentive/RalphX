// Integration test for get_project_stats / compute_project_stats / compute_project_trends
//
// Uses an in-memory SQLite connection with the full migration stack applied,
// then seeds data via raw SQL inserts to verify end-to-end metric computation
// against the real production schema.

use ralphx_lib::commands::metrics_commands::{
    compute_insights_stats, compute_insights_trends, compute_project_stats,
    compute_project_trends,
};
use ralphx_lib::testing::SqliteTestDb;

struct MetricsTestDb {
    _db: SqliteTestDb,
    conn: rusqlite::Connection,
}

impl std::ops::Deref for MetricsTestDb {
    type Target = rusqlite::Connection;

    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}

// ─── Schema helpers ───────────────────────────────────────────────────────────

fn setup_db() -> MetricsTestDb {
    let db = SqliteTestDb::new("metrics-integration");
    let conn = db.new_connection();
    MetricsTestDb { _db: db, conn }
}

fn insert_project(conn: &rusqlite::Connection, id: &str) {
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, format!("Project {id}"), format!("/tmp/test-{id}")],
    )
    .expect("insert project");
}

fn insert_task(conn: &rusqlite::Connection, id: &str, project_id: &str, status: &str) {
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, internal_status, category)
         VALUES (?1, ?2, ?3, ?4, 'regular')",
        rusqlite::params![id, project_id, format!("Task {id}"), status],
    )
    .expect("insert task");
}

fn insert_history(
    conn: &rusqlite::Connection,
    id: &str,
    task_id: &str,
    to_status: &str,
    created_at: &str,
) {
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, changed_by, created_at)
         VALUES (?1, ?2, ?3, 'system', ?4)",
        rusqlite::params![id, task_id, to_status, created_at],
    )
    .expect("insert history");
}

fn insert_review(
    conn: &rusqlite::Connection,
    id: &str,
    project_id: &str,
    task_id: &str,
    status: &str,
) {
    conn.execute(
        "INSERT INTO reviews (id, project_id, task_id, reviewer_type, status)
         VALUES (?1, ?2, ?3, 'ai', ?4)",
        rusqlite::params![id, project_id, task_id, status],
    )
    .expect("insert review");
}

fn insert_step(conn: &rusqlite::Connection, id: &str, task_id: &str) {
    conn.execute(
        "INSERT INTO task_steps (id, task_id, title, status, sort_order)
         VALUES (?1, ?2, 'Step', 'completed', 0)",
        rusqlite::params![id, task_id],
    )
    .expect("insert step");
}

fn insert_merged_history_now(conn: &rusqlite::Connection, id: &str, task_id: &str) {
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, changed_by, created_at)
         VALUES (?1, ?2, 'merged', 'system', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
        rusqlite::params![id, task_id],
    )
    .expect("insert merged history");
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_empty_project_returns_zero_stats() {
    let conn = setup_db();
    insert_project(&conn, "proj1");

    let stats = compute_project_stats(&conn, "proj1", 0, 0).expect("compute_project_stats");

    assert_eq!(stats.task_count, 0);
    assert_eq!(stats.tasks_completed_today, 0);
    assert_eq!(stats.agent_success_rate, 0.0);
    assert_eq!(stats.review_pass_rate, 0.0);
    assert!(stats.cycle_time_breakdown.is_empty());
    assert!(stats.eme.is_none());
}

#[test]
fn test_full_stats_with_all_metric_types() {
    let conn = setup_db();
    insert_project(&conn, "proj1");

    // 3 merged tasks, 1 failed → success rate = 75%
    for i in 1..=3 {
        insert_task(&conn, &format!("t{i}"), "proj1", "merged");
        insert_history(
            &conn,
            &format!("h{i}"),
            &format!("t{i}"),
            "merged",
            &format!("2026-01-0{i}T12:00:00+00:00"),
        );
    }
    insert_task(&conn, "t4", "proj1", "failed");

    // 2 approved reviews, 1 changes_requested → pass rate = 2/3
    insert_review(&conn, "r1", "proj1", "t1", "approved");
    insert_review(&conn, "r2", "proj1", "t2", "approved");
    insert_review(&conn, "r3", "proj1", "t3", "changes_requested");

    let stats = compute_project_stats(&conn, "proj1", 0, 0).expect("compute_project_stats");

    assert_eq!(stats.task_count, 4);
    assert_eq!(stats.agent_success_count, 3);
    assert_eq!(stats.agent_total_count, 4);
    assert!((stats.agent_success_rate - 0.75).abs() < 1e-9);

    assert_eq!(stats.review_pass_count, 2);
    assert_eq!(stats.review_total_count, 3);
    let expected_rate = 2.0 / 3.0;
    assert!((stats.review_pass_rate - expected_rate).abs() < 1e-9);
}

#[test]
fn test_cycle_time_breakdown_uses_real_schema() {
    let conn = setup_db();
    insert_project(&conn, "proj1");

    // Merged task within 90 days with known transitions
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, internal_status, category, updated_at)
         VALUES ('t1', 'proj1', 'T1', 'merged', 'regular', datetime('now', '-1 day'))",
        [],
    )
    .unwrap();

    // ready → executing (1h) → merged
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, changed_by, created_at)
         VALUES ('h1', 't1', 'ready', 'system', '2026-02-01T10:00:00+00:00')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, changed_by, created_at)
         VALUES ('h2', 't1', 'executing', 'system', '2026-02-01T11:00:00+00:00')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, changed_by, created_at)
         VALUES ('h3', 't1', 'merged', 'system', '2026-02-01T12:00:00+00:00')",
        [],
    )
    .unwrap();

    let stats = compute_project_stats(&conn, "proj1", 0, 0).expect("compute_project_stats");

    // ready→executing = 60min, executing→merged = 60min
    assert_eq!(stats.cycle_time_breakdown.len(), 2);
    for phase in &stats.cycle_time_breakdown {
        assert!(
            (phase.avg_minutes - 60.0).abs() < 1.0,
            "phase {} expected 60min, got {}",
            phase.phase,
            phase.avg_minutes
        );
    }
}

#[test]
fn test_eme_returns_none_below_threshold() {
    let conn = setup_db();
    insert_project(&conn, "proj1");

    for i in 1..=4 {
        insert_task(&conn, &format!("t{i}"), "proj1", "merged");
    }

    let stats = compute_project_stats(&conn, "proj1", 0, 0).expect("compute_project_stats");
    assert!(stats.eme.is_none(), "EME should be None for < 5 merged tasks");
}

#[test]
fn test_eme_returns_estimate_at_threshold() {
    let conn = setup_db();
    insert_project(&conn, "proj1");

    // 5 simple tasks (2 steps each, 0 reviews) → Simple tier
    for i in 1..=5 {
        let task_id = format!("t{i}");
        insert_task(&conn, &task_id, "proj1", "merged");
        insert_step(&conn, &format!("s{i}a"), &task_id);
        insert_step(&conn, &format!("s{i}b"), &task_id);
    }

    let stats = compute_project_stats(&conn, "proj1", 0, 0).expect("compute_project_stats");
    let eme = stats.eme.expect("EME should be present for 5+ merged tasks");

    assert_eq!(eme.task_count, 5);
    // Simple: base=1.0 low, ×1.3 = 1.3 high per task → 5 tasks: 5.0 / 6.5
    assert!((eme.low_hours - 5.0).abs() < 0.1, "low_hours={}", eme.low_hours);
    assert!((eme.high_hours - 6.5).abs() < 0.1, "high_hours={}", eme.high_hours);
}

#[test]
fn test_stats_scoped_to_project() {
    let conn = setup_db();
    insert_project(&conn, "proj1");
    insert_project(&conn, "proj2");

    // proj1: 2 merged tasks
    insert_task(&conn, "t1", "proj1", "merged");
    insert_task(&conn, "t2", "proj1", "merged");
    // proj2: 1 failed task
    insert_task(&conn, "t3", "proj2", "failed");
    insert_review(&conn, "r1", "proj2", "t3", "changes_requested");

    let s1 = compute_project_stats(&conn, "proj1", 0, 0).expect("proj1");
    let s2 = compute_project_stats(&conn, "proj2", 0, 0).expect("proj2");

    assert_eq!(s1.task_count, 2);
    assert_eq!(s1.agent_success_count, 2);
    assert_eq!(s1.review_total_count, 0);

    assert_eq!(s2.task_count, 1);
    assert_eq!(s2.agent_success_count, 0);
    assert_eq!(s2.review_total_count, 1);
    assert_eq!(s2.review_pass_count, 0);
}

#[test]
fn test_insights_stats_aggregate_all_projects_from_raw_rows() {
    let conn = setup_db();
    insert_project(&conn, "proj1");
    insert_project(&conn, "proj2");

    for i in 1..=3 {
        let task_id = format!("p1t{i}");
        insert_task(&conn, &task_id, "proj1", "merged");
        insert_step(&conn, &format!("p1s{i}a"), &task_id);
        insert_merged_history_now(&conn, &format!("p1h{i}"), &task_id);
    }
    for i in 1..=2 {
        let task_id = format!("p2t{i}");
        insert_task(&conn, &task_id, "proj2", "merged");
        insert_step(&conn, &format!("p2s{i}a"), &task_id);
        insert_merged_history_now(&conn, &format!("p2h{i}"), &task_id);
    }
    insert_review(&conn, "p1r1", "proj1", "p1t1", "approved");
    insert_review(&conn, "p2r1", "proj2", "p2t1", "changes_requested");

    let stats = compute_insights_stats(&conn, 0, 0).expect("compute_insights_stats");

    assert_eq!(stats.task_count, 5);
    assert_eq!(stats.agent_success_count, 5);
    assert_eq!(stats.agent_total_count, 5);
    assert_eq!(stats.review_pass_count, 1);
    assert_eq!(stats.review_total_count, 2);
    let eme = stats.eme.expect("aggregate EME should use all projects");
    assert_eq!(eme.task_count, 5);
    assert_eq!(eme.scope_label, "All projects task pipeline");
}

// ─── compute_project_trends tests ────────────────────────────────────────────

/// Seed tasks merged this week and verify weekly_throughput includes the current week
/// with the correct count.
///
/// The query uses a recursive CTE that generates the last 12 weeks starting from
/// the most recent Sunday. Tasks are joined by `tasks.internal_status = 'merged'`
/// and `date(updated_at)` falling within the week bucket.
#[test]
fn test_weekly_throughput_counts_merged_tasks_in_current_week() {
    let conn = setup_db();
    insert_project(&conn, "proj1");

    // Insert 3 merged tasks with a merged transition recorded now so they
    // land in the current week bucket used by query_weekly_throughput.
    for i in 1..=3 {
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, internal_status, category, updated_at)
             VALUES (?1, 'proj1', ?2, 'merged', 'regular', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            rusqlite::params![format!("t{i}"), format!("Task {i}")],
        )
        .expect("insert merged task");
        insert_merged_history_now(&conn, &format!("h{i}"), &format!("t{i}"));
    }

    // Also insert a non-merged task to ensure it is NOT counted.
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, internal_status, category, updated_at)
         VALUES ('t_fail', 'proj1', 'Failed task', 'failed', 'regular', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
        [],
    )
    .expect("insert failed task");

    let trends = compute_project_trends(&conn, "proj1", 0, 0).expect("compute_project_trends");

    assert!(
        !trends.weekly_throughput.is_empty(),
        "Merged tasks should produce at least one weekly throughput point"
    );

    // Sum across all weeks — the 3 tasks should contribute exactly 3.0 total.
    let total_throughput: f64 = trends.weekly_throughput.iter().map(|pt| pt.value).sum();
    assert_eq!(
        total_throughput, 3.0,
        "Total weekly throughput across all weeks should be 3 (the 3 merged tasks); \
         got {}. This may indicate the query uses t.status instead of t.internal_status.",
        total_throughput
    );
}

/// Empty project returns no weekly throughput points because leading zero weeks are trimmed.
#[test]
fn test_weekly_throughput_empty_project_returns_twelve_zero_weeks() {
    let conn = setup_db();
    insert_project(&conn, "proj1");

    let trends = compute_project_trends(&conn, "proj1", 0, 0).expect("compute_project_trends");

    assert!(
        trends.weekly_throughput.is_empty(),
        "Empty project should have no weekly throughput points after zero-week trimming"
    );
}

/// Trends are scoped to the requested project and do not bleed across projects.
#[test]
fn test_weekly_throughput_scoped_to_project() {
    let conn = setup_db();
    insert_project(&conn, "proj1");
    insert_project(&conn, "proj2");

    // proj1: 2 merged tasks this week
    for i in 1..=2 {
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, internal_status, category, updated_at)
             VALUES (?1, 'proj1', ?2, 'merged', 'regular', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            rusqlite::params![format!("p1t{i}"), format!("Task {i}")],
        )
        .expect("insert proj1 task");
        insert_merged_history_now(&conn, &format!("p1h{i}"), &format!("p1t{i}"));
    }

    // proj2: 5 merged tasks this week — should not appear in proj1's trends
    for i in 1..=5 {
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, internal_status, category, updated_at)
             VALUES (?1, 'proj2', ?2, 'merged', 'regular', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            rusqlite::params![format!("p2t{i}"), format!("Task {i}")],
        )
        .expect("insert proj2 task");
        insert_merged_history_now(&conn, &format!("p2h{i}"), &format!("p2t{i}"));
    }

    let t1 = compute_project_trends(&conn, "proj1", 0, 0).expect("proj1 trends");
    let t2 = compute_project_trends(&conn, "proj2", 0, 0).expect("proj2 trends");

    // Sum across all weeks: proj1 should total 2.0, proj2 should total 5.0.
    // Using sum avoids having to predict which specific week bucket tasks fall into.
    let p1_total: f64 = t1.weekly_throughput.iter().map(|pt| pt.value).sum();
    let p2_total: f64 = t2.weekly_throughput.iter().map(|pt| pt.value).sum();

    assert_eq!(p1_total, 2.0, "proj1 should have 2 merged tasks total");
    assert_eq!(p2_total, 5.0, "proj2 should have 5 merged tasks total");
}

#[test]
fn test_insights_trends_aggregate_task_and_delivery_throughput() {
    let conn = setup_db();
    insert_project(&conn, "proj1");
    insert_project(&conn, "proj2");

    for i in 1..=2 {
        let task_id = format!("p1t{i}");
        insert_task(&conn, &task_id, "proj1", "merged");
        insert_merged_history_now(&conn, &format!("p1h{i}"), &task_id);
    }
    for i in 1..=3 {
        let task_id = format!("p2t{i}");
        insert_task(&conn, &task_id, "proj2", "merged");
        insert_merged_history_now(&conn, &format!("p2h{i}"), &task_id);
    }

    let trends = compute_insights_trends(&conn, 0, 0).expect("compute_insights_trends");

    let throughput_total: f64 = trends.weekly_throughput.iter().map(|point| point.value).sum();
    let delivery_total: i64 = trends
        .weekly_delivery_throughput
        .iter()
        .map(|point| point.unified_deliveries)
        .sum();

    assert_eq!(throughput_total, 5.0);
    assert_eq!(delivery_total, 5);
}
