// Unit tests for metrics_commands SQL queries.
//
// Each test uses an in-memory SQLite database seeded with known data, then
// asserts the computed values match expectations.  The public API under test
// is `compute_project_stats` and its individual query helpers.

use rusqlite::Connection;

use ralphx_lib::commands::metrics_commands::{
    compute_column_metrics, compute_project_stats, compute_task_metrics,
    invalidate_project_stats_cache, ColumnMetric, ProjectStats, COLUMN_METRICS_CACHE, STATS_CACHE,
};

// ─── Schema helpers ───────────────────────────────────────────────────────────

/// Create the minimal schema required by the metric queries.
fn create_schema(conn: &Connection) {
    conn.execute_batch(
        "
        CREATE TABLE projects (
            id   TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );

        CREATE TABLE tasks (
            id              TEXT PRIMARY KEY,
            project_id      TEXT NOT NULL REFERENCES projects(id),
            internal_status TEXT NOT NULL DEFAULT 'backlog',
            archived_at     TEXT,
            created_at      TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at      TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );

        CREATE TABLE task_state_history (
            id          TEXT PRIMARY KEY,
            task_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            from_status TEXT,
            to_status   TEXT NOT NULL,
            changed_by  TEXT NOT NULL DEFAULT 'system',
            created_at  TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );

        CREATE TABLE task_steps (
            id      TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            status  TEXT NOT NULL DEFAULT 'pending'
        );

        CREATE TABLE reviews (
            id            TEXT PRIMARY KEY,
            project_id    TEXT NOT NULL REFERENCES projects(id),
            task_id       TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            reviewer_type TEXT NOT NULL DEFAULT 'ai',
            status        TEXT NOT NULL DEFAULT 'pending'
        );

        CREATE TABLE project_metrics_config (
            project_id            TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
            simple_base_hours     REAL NOT NULL DEFAULT 2.0,
            medium_base_hours     REAL NOT NULL DEFAULT 4.0,
            complex_base_hours    REAL NOT NULL DEFAULT 8.0,
            calendar_factor       REAL NOT NULL DEFAULT 1.5,
            working_days_per_week INTEGER NOT NULL DEFAULT 5,
            updated_at            TEXT
        );
        ",
    )
    .expect("create schema");
}

fn insert_project(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO projects (id, name) VALUES (?1, ?2)",
        rusqlite::params![id, format!("Project {id}")],
    )
    .unwrap();
}

fn insert_task(conn: &Connection, id: &str, project_id: &str, status: &str) {
    conn.execute(
        "INSERT INTO tasks (id, project_id, internal_status) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, project_id, status],
    )
    .unwrap();
}

fn insert_history(
    conn: &Connection,
    id: &str,
    task_id: &str,
    to_status: &str,
    created_at: &str,
) {
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, task_id, to_status, created_at],
    )
    .unwrap();
}

fn insert_review(conn: &Connection, id: &str, project_id: &str, task_id: &str, status: &str) {
    conn.execute(
        "INSERT INTO reviews (id, project_id, task_id, status) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, project_id, task_id, status],
    )
    .unwrap();
}

fn insert_step(conn: &Connection, id: &str, task_id: &str) {
    conn.execute(
        "INSERT INTO task_steps (id, task_id) VALUES (?1, ?2)",
        rusqlite::params![id, task_id],
    )
    .unwrap();
}

// ─── Edge cases ───────────────────────────────────────────────────────────────

#[test]
fn test_zero_tasks_returns_zero_metrics() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();

    assert_eq!(stats.task_count, 0);
    assert_eq!(stats.tasks_completed_today, 0);
    assert_eq!(stats.tasks_completed_this_week, 0);
    assert_eq!(stats.tasks_completed_this_month, 0);
    assert_eq!(stats.agent_success_rate, 0.0);
    assert_eq!(stats.agent_success_count, 0);
    assert_eq!(stats.agent_total_count, 0);
    assert_eq!(stats.review_pass_rate, 0.0);
    assert_eq!(stats.review_pass_count, 0);
    assert_eq!(stats.review_total_count, 0);
    assert!(stats.cycle_time_breakdown.is_empty());
    assert!(stats.column_dwell_times.is_empty());
    assert!(stats.eme.is_none());
}

#[test]
fn test_single_merged_task_no_eme() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");
    insert_task(&conn, "t1", "proj1", "merged");
    insert_history(&conn, "h1", "t1", "merged", "2099-01-01T12:00:00+00:00");

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();

    assert_eq!(stats.task_count, 1);
    assert_eq!(stats.agent_success_count, 1);
    assert_eq!(stats.agent_total_count, 1);
    assert!((stats.agent_success_rate - 1.0).abs() < 1e-9);
    // EME requires ≥ 5 merged tasks
    assert!(stats.eme.is_none());
}

#[test]
fn test_all_cancelled_tasks() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");
    insert_task(&conn, "t1", "proj1", "cancelled");
    insert_task(&conn, "t2", "proj1", "cancelled");

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();

    assert_eq!(stats.task_count, 2);
    assert_eq!(stats.agent_success_count, 0);
    assert_eq!(stats.agent_total_count, 2);
    assert_eq!(stats.agent_success_rate, 0.0);
}

#[test]
fn test_no_reviews_returns_zero_pass_rate() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");
    insert_task(&conn, "t1", "proj1", "merged");

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();

    assert_eq!(stats.review_pass_rate, 0.0);
    assert_eq!(stats.review_pass_count, 0);
    assert_eq!(stats.review_total_count, 0);
}

// ─── Metric correctness ───────────────────────────────────────────────────────

#[test]
fn test_agent_success_rate_partial() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");
    insert_task(&conn, "t1", "proj1", "merged");
    insert_task(&conn, "t2", "proj1", "merged");
    insert_task(&conn, "t3", "proj1", "failed");
    insert_task(&conn, "t4", "proj1", "cancelled");

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();

    assert_eq!(stats.agent_success_count, 2);
    assert_eq!(stats.agent_total_count, 4);
    assert!((stats.agent_success_rate - 0.5).abs() < 1e-9);
}

#[test]
fn test_review_pass_rate() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");
    insert_task(&conn, "t1", "proj1", "merged");
    insert_review(&conn, "r1", "proj1", "t1", "approved");
    insert_review(&conn, "r2", "proj1", "t1", "approved");
    insert_review(&conn, "r3", "proj1", "t1", "changes_requested");

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();

    assert_eq!(stats.review_pass_count, 2);
    assert_eq!(stats.review_total_count, 3);
    let expected = 2.0 / 3.0;
    assert!((stats.review_pass_rate - expected).abs() < 1e-9);
}

#[test]
fn test_tasks_completed_daily_weekly_monthly_windows() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    // Task merged at 1am today (UTC) — counts in today, week, month.
    // 'start of day' + 1 hour avoids the midnight edge case (reliable on all UTC hours).
    insert_task(&conn, "t1", "proj1", "merged");
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, created_at)
         VALUES ('h1', 't1', 'merged', datetime('now', 'start of day', '+1 hour'))",
        [],
    )
    .unwrap();

    // Task merged 15 days ago — month only (always outside the 0–6 day calendar-week window).
    insert_task(&conn, "t2", "proj1", "merged");
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, created_at)
         VALUES ('h2', 't2', 'merged', datetime('now', '-15 days'))",
        [],
    )
    .unwrap();

    // Task merged 45 days ago — outside all windows
    insert_task(&conn, "t3", "proj1", "merged");
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, created_at)
         VALUES ('h3', 't3', 'merged', datetime('now', '-45 days'))",
        [],
    )
    .unwrap();

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();

    assert_eq!(stats.tasks_completed_today, 1); // t1 only
    assert_eq!(stats.tasks_completed_this_week, 1); // t1 only (15+ days ago is never in the week window)
    assert_eq!(stats.tasks_completed_this_month, 2); // t1 + t2
}

#[test]
fn test_cycle_time_breakdown_lag_window_function() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    // One merged task with known transition timestamps (1 hour in each phase)
    conn.execute(
        "INSERT INTO tasks (id, project_id, internal_status, updated_at)
         VALUES ('t1', 'proj1', 'merged', datetime('now', '-1 day'))",
        [],
    )
    .unwrap();

    // State: ready → executing (1h) → pending_review (1h) → merged
    let transitions: &[(&str, &str, &str)] = &[
        ("h1", "ready", "2026-01-01T10:00:00+00:00"),
        ("h2", "executing", "2026-01-01T11:00:00+00:00"),
        ("h3", "pending_review", "2026-01-01T12:00:00+00:00"),
        ("h4", "merged", "2026-01-01T13:00:00+00:00"),
    ];
    for (i, (_, to_status, created_at)) in transitions.iter().enumerate() {
        conn.execute(
            "INSERT INTO task_state_history (id, task_id, to_status, created_at)
             VALUES (?1, 't1', ?2, ?3)",
            rusqlite::params![format!("h{}", i + 1), to_status, created_at],
        )
        .unwrap();
    }

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();

    // Should have 3 phases (ready→executing, executing→pending_review, pending_review→merged)
    // Each phase = 60 minutes
    assert_eq!(stats.cycle_time_breakdown.len(), 3);
    for phase in &stats.cycle_time_breakdown {
        assert!(
            (phase.avg_minutes - 60.0).abs() < 1.0,
            "phase {} avg_minutes={} expected ~60",
            phase.phase,
            phase.avg_minutes
        );
        assert_eq!(phase.sample_size, 1);
    }
}

#[test]
fn test_cycle_time_90_day_filter_excludes_old_tasks() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    // Task merged 100 days ago — should be excluded from cycle time
    conn.execute(
        "INSERT INTO tasks (id, project_id, internal_status, updated_at)
         VALUES ('t1', 'proj1', 'merged', datetime('now', '-100 days'))",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, created_at)
         VALUES ('h1', 't1', 'merged', datetime('now', '-100 days'))",
        [],
    )
    .unwrap();

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    assert!(stats.cycle_time_breakdown.is_empty());
}

#[test]
fn test_eme_simple_tier_5_tasks() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    // 5 merged tasks, each with 1 step, 0 reviews → Simple tier (base 1h, calendar 1.3)
    for i in 1..=5 {
        let task_id = format!("t{i}");
        insert_task(&conn, &task_id, "proj1", "merged");
        insert_step(&conn, &format!("s{i}"), &task_id);
    }

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    let eme = stats.eme.expect("EME should be present for 5+ tasks");

    assert_eq!(eme.task_count, 5);
    // Simple: base=1.0 low, ×1.3 = 1.3 high per task → 5 tasks: 5.0 / 6.5
    assert!((eme.low_hours - 5.0).abs() < 0.1, "low_hours={}", eme.low_hours);
    assert!((eme.high_hours - 6.5).abs() < 0.1, "high_hours={}", eme.high_hours);
}

#[test]
fn test_eme_mixed_tiers() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    // Simple task (2 steps, 0 reviews): low=1, high=1.3
    insert_task(&conn, "t1", "proj1", "merged");
    insert_step(&conn, "s1a", "t1");
    insert_step(&conn, "s1b", "t1");

    // Medium task (5 steps, 0 reviews): low=2, high=2.6
    insert_task(&conn, "t2", "proj1", "merged");
    for j in 1..=5 {
        insert_step(&conn, &format!("s2{j}"), "t2");
    }

    // Complex task (8 steps, 0 reviews): low=4, high=5.2
    insert_task(&conn, "t3", "proj1", "merged");
    for j in 1..=8 {
        insert_step(&conn, &format!("s3{j}"), "t3");
    }

    // 3 simple tasks to reach the 5-task threshold
    for i in 4..=6 {
        let task_id = format!("t{i}");
        insert_task(&conn, &task_id, "proj1", "merged");
    }

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    let eme = stats.eme.expect("EME should be present");

    // Senior defaults: simple=1.0, medium=2.0, complex=4.0, calendar=1.3
    // t1 simple: 1/1.3, t2 medium: 2/2.6, t3 complex: 4/5.2, t4-t6 simple: 1/1.3 each
    // total low = 1 + 2 + 4 + 1 + 1 + 1 = 10.0
    // total high = 1.3 + 2.6 + 5.2 + 1.3 + 1.3 + 1.3 = 13.0
    assert_eq!(eme.task_count, 6);
    assert!((eme.low_hours - 10.0).abs() < 0.5, "low_hours={}", eme.low_hours);
    assert!((eme.high_hours - 13.0).abs() < 0.5, "high_hours={}", eme.high_hours);
}

#[test]
fn test_eme_review_cycle_bumps_tier() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    // Task with 1 step (normally simple) but 1 review → medium tier
    for i in 1..=5 {
        let task_id = format!("t{i}");
        insert_task(&conn, &task_id, "proj1", "merged");
        insert_step(&conn, &format!("s{i}"), &task_id);
        insert_review(&conn, &format!("r{i}"), "proj1", &task_id, "approved");
    }

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    let eme = stats.eme.expect("EME present");

    // Senior defaults: Medium base=2.0, calendar=1.3
    // low=2.0, high=2.0×1.3=2.6 per task → 5×: 10.0/13.0
    assert!((eme.low_hours - 10.0).abs() < 0.5, "low_hours={}", eme.low_hours);
    assert!((eme.high_hours - 13.0).abs() < 0.5, "high_hours={}", eme.high_hours);
}

#[test]
fn test_eme_fewer_than_5_tasks_returns_none() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    for i in 1..=4 {
        insert_task(&conn, &format!("t{i}"), "proj1", "merged");
    }

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    assert!(stats.eme.is_none());
}

#[test]
fn test_archived_tasks_excluded_from_task_count() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    insert_task(&conn, "t1", "proj1", "merged");
    // Insert an archived task
    conn.execute(
        "INSERT INTO tasks (id, project_id, internal_status, archived_at)
         VALUES ('t2', 'proj1', 'cancelled', '2026-01-01T00:00:00+00:00')",
        [],
    )
    .unwrap();

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    assert_eq!(stats.task_count, 1, "archived task should not be counted");
    // Archived cancelled tasks should not count in terminal totals either
    assert_eq!(stats.agent_total_count, 1);
}

#[test]
fn test_different_projects_isolated() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");
    insert_project(&conn, "proj2");

    insert_task(&conn, "t1", "proj1", "merged");
    insert_task(&conn, "t2", "proj2", "failed");

    let stats1 = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    let stats2 = compute_project_stats(&conn, "proj2", 0, 0).unwrap();

    assert_eq!(stats1.task_count, 1);
    assert_eq!(stats1.agent_success_count, 1);
    assert_eq!(stats2.task_count, 1);
    assert_eq!(stats2.agent_success_count, 0);
}

// ─── MetricsConfig override tests ────────────────────────────────────────────

#[test]
fn test_eme_uses_default_config_when_no_override() {
    // Existing behavior unchanged — this tests that the load_metrics_config fallback works
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    for i in 1..=5 {
        let task_id = format!("t{i}");
        insert_task(&conn, &task_id, "proj1", "merged");
        insert_step(&conn, &format!("s{i}"), &task_id);
    }

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    let eme = stats.eme.expect("EME should be present");

    // Senior default: Simple base=1.0, calendar=1.3 → low=1.0, high=1.3 per task → 5×: 5.0/6.5
    assert!((eme.low_hours - 5.0).abs() < 0.1);
    assert!((eme.high_hours - 6.5).abs() < 0.1);
}

#[test]
fn test_eme_uses_custom_base_hours_override() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    // Insert custom config: simple_base_hours=4.0 (doubled)
    conn.execute(
        "INSERT INTO project_metrics_config (project_id, simple_base_hours, medium_base_hours, complex_base_hours, calendar_factor) VALUES ('proj1', 4.0, 8.0, 16.0, 2.0)",
        [],
    ).unwrap();

    for i in 1..=5 {
        let task_id = format!("t{i}");
        insert_task(&conn, &task_id, "proj1", "merged");
        insert_step(&conn, &format!("s{i}"), &task_id);
    }

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    let eme = stats.eme.expect("EME should be present");

    // Custom: Simple 1.0 × 4.0 = 4.0 low, ×2.0 = 8.0 high per task → 5×: 20.0/40.0
    assert!((eme.low_hours - 20.0).abs() < 0.1, "low_hours={}", eme.low_hours);
    assert!((eme.high_hours - 40.0).abs() < 0.1, "high_hours={}", eme.high_hours);
}

#[test]
fn test_eme_calendar_factor_override() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    // Override only calendar_factor (keep base hours at Senior defaults, but calendar=2.0)
    conn.execute(
        "INSERT INTO project_metrics_config (project_id, simple_base_hours, medium_base_hours, complex_base_hours, calendar_factor) VALUES ('proj1', 1.0, 2.0, 4.0, 2.0)",
        [],
    ).unwrap();

    for i in 1..=5 {
        let task_id = format!("t{i}");
        insert_task(&conn, &task_id, "proj1", "merged");
        insert_step(&conn, &format!("s{i}"), &task_id);
    }

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    let eme = stats.eme.expect("EME present");

    // Simple base=1.0 low, ×2.0 = 2.0 high per task → 5×: 5.0/10.0
    assert!((eme.low_hours - 5.0).abs() < 0.1, "low_hours={}", eme.low_hours);
    assert!((eme.high_hours - 10.0).abs() < 0.1, "high_hours={}", eme.high_hours);
}

#[test]
fn test_different_projects_use_independent_configs() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");
    insert_project(&conn, "proj2");

    // proj1 has custom config
    conn.execute(
        "INSERT INTO project_metrics_config (project_id, simple_base_hours, medium_base_hours, complex_base_hours, calendar_factor) VALUES ('proj1', 4.0, 8.0, 16.0, 1.0)",
        [],
    ).unwrap();
    // proj2 uses defaults

    for proj in &["proj1", "proj2"] {
        for i in 1..=5 {
            let task_id = format!("{proj}-t{i}");
            insert_task(&conn, &task_id, proj, "merged");
            insert_step(&conn, &format!("{proj}-s{i}"), &task_id);
        }
    }

    let stats1 = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    let stats2 = compute_project_stats(&conn, "proj2", 0, 0).unwrap();
    let eme1 = stats1.eme.unwrap();
    let eme2 = stats2.eme.unwrap();

    // proj1: Simple 1.0 × 4.0 = 4.0 low, ×1.0 = 4.0 high → 5×: 20.0/20.0
    assert!((eme1.low_hours - 20.0).abs() < 0.1, "proj1 low={}", eme1.low_hours);
    assert!((eme1.high_hours - 20.0).abs() < 0.1, "proj1 high={}", eme1.high_hours);

    // proj2: Senior default Simple base=1.0, calendar=1.3 → low=1.0, high=1.3 per task → 5×: 5.0/6.5
    assert!((eme2.low_hours - 5.0).abs() < 0.1, "proj2 low={}", eme2.low_hours);
    assert!((eme2.high_hours - 6.5).abs() < 0.1, "proj2 high={}", eme2.high_hours);
}

// ─── Cache invalidation ───────────────────────────────────────────────────────

#[test]
fn test_invalidate_project_stats_cache_removes_entry() {
    use std::time::Instant;

    let project_id = "cache-test-proj";
    // Manually insert a fake entry
    let fake_stats = ProjectStats {
        task_count: 99,
        tasks_completed_today: 0,
        tasks_completed_this_week: 0,
        tasks_completed_this_month: 0,
        agent_success_rate: 0.0,
        agent_success_count: 0,
        agent_total_count: 0,
        review_pass_rate: 0.0,
        review_pass_count: 0,
        review_total_count: 0,
        cycle_time_breakdown: vec![],
        column_dwell_times: vec![],
        avg_pipeline_minutes: None,
        eme: None,
    };
    let cache_key = format!("project:{project_id}:0:0");
    let all_cache_key = "all:0:0".to_string();
    STATS_CACHE.insert(cache_key.clone(), (Instant::now(), fake_stats.clone()));
    STATS_CACHE.insert(all_cache_key.clone(), (Instant::now(), fake_stats));

    assert!(STATS_CACHE.contains_key(&cache_key));
    assert!(STATS_CACHE.contains_key(&all_cache_key));

    invalidate_project_stats_cache(project_id);

    assert!(!STATS_CACHE.contains_key(&cache_key), "cache entry should be evicted");
    assert!(
        !STATS_CACHE.contains_key(&all_cache_key),
        "all-project cache entry should be evicted"
    );
}

#[test]
fn test_invalidate_also_clears_column_metrics_cache() {
    use std::time::Instant;

    let project_id = "column-cache-test-proj";
    let fake_metrics = vec![ColumnMetric {
        column_id: "backlog".to_string(),
        column_name: "Backlog".to_string(),
        task_count: 5,
        avg_age_hours: 2.0,
    }];
    COLUMN_METRICS_CACHE.insert(project_id.to_string(), (Instant::now(), fake_metrics));

    assert!(COLUMN_METRICS_CACHE.contains_key(project_id));

    invalidate_project_stats_cache(project_id);

    assert!(!COLUMN_METRICS_CACHE.contains_key(project_id), "column metrics cache should also be evicted");
}

// ─── Column metrics ───────────────────────────────────────────────────────────

#[test]
fn test_column_metrics_empty_project() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    let metrics = compute_column_metrics(&conn, "proj1").unwrap();

    // Always returns 5 fixed columns, all with zero counts
    assert_eq!(metrics.len(), 5);
    for col in &metrics {
        assert_eq!(col.task_count, 0, "column {} should have 0 tasks", col.column_id);
        assert_eq!(col.avg_age_hours, 0.0, "column {} should have 0 avg age", col.column_id);
    }
}

#[test]
fn test_column_metrics_tasks_distributed_across_columns() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    insert_task(&conn, "t1", "proj1", "backlog");
    insert_task(&conn, "t2", "proj1", "ready");
    insert_task(&conn, "t3", "proj1", "executing");
    insert_task(&conn, "t4", "proj1", "pending_review");
    insert_task(&conn, "t5", "proj1", "merged");

    let metrics = compute_column_metrics(&conn, "proj1").unwrap();

    let backlog = metrics.iter().find(|m| m.column_id == "backlog").unwrap();
    let ready = metrics.iter().find(|m| m.column_id == "ready").unwrap();
    let in_progress = metrics.iter().find(|m| m.column_id == "in_progress").unwrap();
    let in_review = metrics.iter().find(|m| m.column_id == "in_review").unwrap();
    let done = metrics.iter().find(|m| m.column_id == "done").unwrap();

    assert_eq!(backlog.task_count, 1);
    assert_eq!(ready.task_count, 1);
    assert_eq!(in_progress.task_count, 1);
    assert_eq!(in_review.task_count, 1);
    assert_eq!(done.task_count, 1);
}

#[test]
fn test_column_metrics_archived_tasks_excluded() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    insert_task(&conn, "t1", "proj1", "backlog");
    // Archived task — should not count
    conn.execute(
        "INSERT INTO tasks (id, project_id, internal_status, archived_at)
         VALUES ('t2', 'proj1', 'backlog', '2026-01-01T00:00:00+00:00')",
        [],
    )
    .unwrap();

    let metrics = compute_column_metrics(&conn, "proj1").unwrap();
    let backlog = metrics.iter().find(|m| m.column_id == "backlog").unwrap();
    assert_eq!(backlog.task_count, 1, "archived task must not count");
}

#[test]
fn test_column_metrics_revision_needed_in_ready_column() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    insert_task(&conn, "t1", "proj1", "ready");
    insert_task(&conn, "t2", "proj1", "revision_needed");

    let metrics = compute_column_metrics(&conn, "proj1").unwrap();
    let ready = metrics.iter().find(|m| m.column_id == "ready").unwrap();
    assert_eq!(ready.task_count, 2, "revision_needed should be in the ready column");
}

// ─── Task metrics ─────────────────────────────────────────────────────────────

#[test]
fn test_task_metrics_empty_task() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");
    insert_task(&conn, "t1", "proj1", "ready");

    let metrics = compute_task_metrics(&conn, "t1").unwrap();

    assert_eq!(metrics.step_count, 0);
    assert_eq!(metrics.completed_step_count, 0);
    assert_eq!(metrics.review_count, 0);
    assert_eq!(metrics.approved_review_count, 0);
    assert_eq!(metrics.execution_minutes, 0.0);
}

#[test]
fn test_task_metrics_step_counts() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");
    insert_task(&conn, "t1", "proj1", "executing");

    insert_step(&conn, "s1", "t1");
    insert_step(&conn, "s2", "t1");
    // Mark s1 completed
    conn.execute(
        "UPDATE task_steps SET status = 'completed' WHERE id = 's1'",
        [],
    )
    .unwrap();

    let metrics = compute_task_metrics(&conn, "t1").unwrap();

    assert_eq!(metrics.step_count, 2);
    assert_eq!(metrics.completed_step_count, 1);
}

#[test]
fn test_task_metrics_review_counts() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");
    insert_task(&conn, "t1", "proj1", "merged");

    insert_review(&conn, "r1", "proj1", "t1", "approved");
    insert_review(&conn, "r2", "proj1", "t1", "changes_requested");
    insert_review(&conn, "r3", "proj1", "t1", "approved");

    let metrics = compute_task_metrics(&conn, "t1").unwrap();

    assert_eq!(metrics.review_count, 3);
    assert_eq!(metrics.approved_review_count, 2);
}

#[test]
fn test_task_metrics_execution_minutes() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");
    insert_task(&conn, "t1", "proj1", "merged");

    // Transition: ready → executing (spend 30 min) → merged
    conn.execute(
        "INSERT INTO task_state_history (id, task_id, to_status, created_at)
         VALUES
           ('h1', 't1', 'executing',   '2026-01-01T10:00:00+00:00'),
           ('h2', 't1', 'merged',      '2026-01-01T10:30:00+00:00')",
        [],
    )
    .unwrap();

    let metrics = compute_task_metrics(&conn, "t1").unwrap();

    // executing phase lasted 30 minutes
    assert!(
        (metrics.execution_minutes - 30.0).abs() < 1.0,
        "expected ~30 execution_minutes, got {}",
        metrics.execution_minutes
    );
}

// ─── Column dwell time ───────────────────────────────────────────────────────

#[test]
fn test_column_dwell_times_empty_project() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    assert!(stats.column_dwell_times.is_empty());
}

#[test]
fn test_column_dwell_times_maps_states_to_columns() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    // One merged task with known transitions: ready(1h) → executing(2h) → pending_review(30m) → merged
    conn.execute(
        "INSERT INTO tasks (id, project_id, internal_status, updated_at)
         VALUES ('t1', 'proj1', 'merged', datetime('now', '-1 day'))",
        [],
    )
    .unwrap();

    insert_history(&conn, "dw1", "t1", "ready", "2026-01-01T10:00:00+00:00");
    insert_history(&conn, "dw2", "t1", "executing", "2026-01-01T11:00:00+00:00");
    insert_history(&conn, "dw3", "t1", "pending_review", "2026-01-01T13:00:00+00:00");
    insert_history(&conn, "dw4", "t1", "merged", "2026-01-01T13:30:00+00:00");

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    let dwell = &stats.column_dwell_times;

    // Should have: ready(60m), in_progress(120m), in_review(30m)
    assert_eq!(dwell.len(), 3, "expected 3 columns with dwell times");

    let ready = dwell.iter().find(|d| d.column_id == "ready").expect("ready column");
    assert!((ready.avg_minutes - 60.0).abs() < 1.0, "ready avg={}", ready.avg_minutes);

    let in_progress = dwell.iter().find(|d| d.column_id == "in_progress").expect("in_progress column");
    assert!((in_progress.avg_minutes - 120.0).abs() < 1.0, "in_progress avg={}", in_progress.avg_minutes);

    let in_review = dwell.iter().find(|d| d.column_id == "in_review").expect("in_review column");
    assert!((in_review.avg_minutes - 30.0).abs() < 1.0, "in_review avg={}", in_review.avg_minutes);
}

#[test]
fn test_column_dwell_times_averages_across_tasks() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_project(&conn, "proj1");

    // Task 1: ready for 60m
    conn.execute(
        "INSERT INTO tasks (id, project_id, internal_status, updated_at)
         VALUES ('t1', 'proj1', 'merged', datetime('now', '-1 day'))",
        [],
    )
    .unwrap();
    insert_history(&conn, "a1", "t1", "ready", "2026-01-01T10:00:00+00:00");
    insert_history(&conn, "a2", "t1", "executing", "2026-01-01T11:00:00+00:00");
    insert_history(&conn, "a3", "t1", "merged", "2026-01-01T12:00:00+00:00");

    // Task 2: ready for 120m
    conn.execute(
        "INSERT INTO tasks (id, project_id, internal_status, updated_at)
         VALUES ('t2', 'proj1', 'merged', datetime('now', '-1 day'))",
        [],
    )
    .unwrap();
    insert_history(&conn, "b1", "t2", "ready", "2026-01-02T10:00:00+00:00");
    insert_history(&conn, "b2", "t2", "executing", "2026-01-02T12:00:00+00:00");
    insert_history(&conn, "b3", "t2", "merged", "2026-01-02T13:00:00+00:00");

    let stats = compute_project_stats(&conn, "proj1", 0, 0).unwrap();
    let ready = stats.column_dwell_times.iter().find(|d| d.column_id == "ready").expect("ready");

    // Average of 60m and 120m = 90m
    assert!((ready.avg_minutes - 90.0).abs() < 1.0, "ready avg={}", ready.avg_minutes);
    assert_eq!(ready.sample_size, 2);
}
