// Private SQL query helpers for project stats metrics.
// Called by compute_project_stats and compute_column_metrics in metrics_commands.rs.

use rusqlite::params;

use crate::commands::metrics_commands::{load_metrics_config, MetricsConfig};
use crate::commands::metrics_types::{
    ColumnDwellTime, ColumnMetric, CycleTimePhase, EmeEstimate, ProjectStats, TaskMetrics,
};
use crate::error::{AppError, AppResult};

// ─── Project stats queries ─────────────────────────────────────────────────────

/// Total non-archived tasks in the project (all statuses).
pub(crate) fn query_task_count(conn: &rusqlite::Connection, project_id: &str) -> AppResult<i64> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE project_id = ?1 AND archived_at IS NULL",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(count)
}

/// Tasks that reached `merged` status in the last day / current calendar week / last 30 days.
/// Uses `task_state_history` for accurate merge timestamps.
/// `week_start_day`: 0=Sunday, 1=Monday, ..., 6=Saturday.
/// `tz_offset_minutes`: minutes east of UTC (e.g., AEST=+660, EST=-300).
pub(crate) fn query_tasks_completed(
    conn: &rusqlite::Connection,
    project_id: &str,
    week_start_day: u8,
    tz_offset_minutes: i32,
) -> AppResult<(i64, i64, i64)> {
    let wt = (week_start_day + 6) % 7;
    let tz_off = format!("{:+} minutes", tz_offset_minutes);
    let tz_neg = format!("{:+} minutes", -tz_offset_minutes);
    let sql = format!(
        "SELECT
            COUNT(CASE WHEN datetime(h.created_at) >= datetime(date('now', '{tz_off}'), '{tz_neg}') THEN 1 END) as today,
            COUNT(CASE WHEN datetime(h.created_at) >= datetime(date('now', '{tz_off}', 'weekday {wt}', '-6 days'), '{tz_neg}') THEN 1 END) as this_week,
            COUNT(CASE WHEN datetime(h.created_at) >= datetime('now', '-30 days') THEN 1 END) as this_month
        FROM task_state_history h
        JOIN tasks t ON t.id = h.task_id
        WHERE t.project_id = ?1
          AND h.to_status = 'merged'"
    );
    let (today, week, month) = conn
        .query_row(&sql, params![project_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
        })
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok((today, week, month))
}

/// Agent success rate: merged / (merged + failed + cancelled + stopped).
pub(crate) fn query_agent_success_rate(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> AppResult<(f64, i64, i64)> {
    let sql = "
        SELECT
            COUNT(CASE WHEN internal_status = 'merged' THEN 1 END)                                   AS success,
            COUNT(CASE WHEN internal_status IN ('merged','failed','cancelled','stopped') THEN 1 END) AS total
        FROM tasks
        WHERE project_id = ?1
          AND archived_at IS NULL
    ";
    let (success, total) = conn
        .query_row(sql, params![project_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| AppError::Database(e.to_string()))?;

    let rate = if total == 0 {
        0.0
    } else {
        success as f64 / total as f64
    };
    Ok((rate, success, total))
}

/// Review pass rate: approved / (approved + changes_requested).
pub(crate) fn query_review_pass_rate(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> AppResult<(f64, i64, i64)> {
    let sql = "
        SELECT
            COUNT(CASE WHEN r.status = 'approved' THEN 1 END)                                            AS passed,
            COUNT(CASE WHEN r.status IN ('approved','changes_requested') THEN 1 END)                     AS total
        FROM reviews r
        WHERE r.project_id = ?1
    ";
    let (passed, total) = conn
        .query_row(sql, params![project_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| AppError::Database(e.to_string()))?;

    let rate = if total == 0 {
        0.0
    } else {
        passed as f64 / total as f64
    };
    Ok((rate, passed, total))
}

/// Cycle time breakdown per phase using LAG() window function.
/// Only considers tasks merged in the last 90 days.
pub(crate) fn query_cycle_time_breakdown(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> AppResult<Vec<CycleTimePhase>> {
    let sql = "
        WITH merged_tasks AS (
            SELECT id FROM tasks
            WHERE project_id = ?1
              AND internal_status = 'merged'
              AND datetime(updated_at) >= datetime('now', '-90 days')
        ),
        transitions AS (
            SELECT
                h.task_id,
                h.to_status,
                h.created_at,
                LAG(h.created_at)  OVER (PARTITION BY h.task_id ORDER BY h.created_at) AS prev_at,
                LAG(h.to_status)   OVER (PARTITION BY h.task_id ORDER BY h.created_at) AS prev_status
            FROM task_state_history h
            WHERE h.task_id IN (SELECT id FROM merged_tasks)
        )
        SELECT
            prev_status                                                               AS phase,
            AVG((julianday(created_at) - julianday(prev_at)) * 24.0 * 60.0)          AS avg_minutes,
            COUNT(*)                                                                  AS sample_size
        FROM transitions
        WHERE prev_at IS NOT NULL
          AND prev_status IS NOT NULL
        GROUP BY prev_status
        ORDER BY avg_minutes DESC
    ";

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| AppError::Database(e.to_string()))?;

    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok(CycleTimePhase {
                phase: row.get::<_, String>(0)?,
                avg_minutes: row.get::<_, f64>(1)?,
                sample_size: row.get::<_, i64>(2)?,
            })
        })
        .map_err(|e| AppError::Database(e.to_string()))?;

    let mut phases = Vec::new();
    for row in rows {
        phases.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }
    Ok(phases)
}

/// Estimated Manual Effort (EME) range.
/// Returns `None` when < 5 merged tasks exist.
///
/// Uses user-calibrated config for base hours per complexity tier.
pub(crate) fn query_eme(
    conn: &rusqlite::Connection,
    project_id: &str,
    config: &MetricsConfig,
) -> AppResult<Option<EmeEstimate>> {
    let sql = "
        SELECT
            t.id,
            COALESCE(s.step_count,   0) AS step_count,
            COALESCE(r.review_count, 0) AS review_cycles,
            date(t.updated_at)          AS merged_date
        FROM tasks t
        LEFT JOIN (
            SELECT task_id, COUNT(*) AS step_count
            FROM task_steps
            GROUP BY task_id
        ) s ON s.task_id = t.id
        LEFT JOIN (
            SELECT task_id, COUNT(*) AS review_count
            FROM reviews
            GROUP BY task_id
        ) r ON r.task_id = t.id
        WHERE t.project_id = ?1
          AND t.internal_status = 'merged'
          AND t.archived_at IS NULL
        ORDER BY t.updated_at
    ";

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| AppError::Database(e.to_string()))?;

    let rows = stmt
        .query_map(params![project_id], |row| {
            let step_count: i64 = row.get(1)?;
            let review_cycles: i64 = row.get(2)?;
            let merged_date: Option<String> = row.get(3)?;
            Ok((step_count, review_cycles, merged_date))
        })
        .map_err(|e| AppError::Database(e.to_string()))?;

    let mut task_rows: Vec<(i64, i64, Option<String>)> = Vec::new();
    for row in rows {
        task_rows.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }

    if task_rows.len() < 5 {
        return Ok(None);
    }

    let earliest = task_rows.first().and_then(|r| r.2.clone());
    let latest = task_rows.last().and_then(|r| r.2.clone());

    let (low_total, high_total) = task_rows.iter().fold((0.0f64, 0.0f64), |acc, (steps, reviews, _)| {
        let (_weight, base_hours) = complexity_tier(*steps, *reviews, config);
        let low = base_hours;
        let high = base_hours * config.calendar_factor;
        (acc.0 + low, acc.1 + high)
    });

    Ok(Some(EmeEstimate {
        low_hours: (low_total * 10.0).round() / 10.0,
        high_hours: (high_total * 10.0).round() / 10.0,
        task_count: task_rows.len() as i64,
        earliest_task_date: earliest,
        latest_task_date: latest,
    }))
}

/// Returns (weight, base_hours) for a complexity tier using user-calibrated config.
/// Weights (1.0/2.5/5.0) are fixed multipliers; only base_hours are user-adjustable.
fn complexity_tier(step_count: i64, review_cycles: i64, config: &MetricsConfig) -> (f64, f64) {
    if step_count >= 8 || review_cycles >= 2 {
        (5.0, config.complex_base_hours)
    } else if step_count >= 4 || review_cycles == 1 {
        (2.5, config.medium_base_hours)
    } else {
        (1.0, config.simple_base_hours)
    }
}

/// Average dwell time per Kanban column using LAG() window function.
/// Maps internal statuses to Kanban columns and aggregates dwell time.
/// Only considers tasks merged in the last 90 days.
pub(crate) fn query_column_dwell_times(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> AppResult<Vec<ColumnDwellTime>> {
    let sql = "
        WITH merged_tasks AS (
            SELECT id FROM tasks
            WHERE project_id = ?1
              AND internal_status = 'merged'
              AND datetime(updated_at) >= datetime('now', '-90 days')
        ),
        transitions AS (
            SELECT
                h.task_id,
                h.to_status,
                h.created_at,
                LAG(h.created_at)  OVER (PARTITION BY h.task_id ORDER BY h.created_at) AS prev_at,
                LAG(h.to_status)   OVER (PARTITION BY h.task_id ORDER BY h.created_at) AS prev_status
            FROM task_state_history h
            WHERE h.task_id IN (SELECT id FROM merged_tasks)
        ),
        column_mapped AS (
            SELECT
                CASE
                    WHEN prev_status IN ('ready') THEN 'ready'
                    WHEN prev_status IN ('executing', 're_executing') THEN 'in_progress'
                    WHEN prev_status IN ('pending_review', 'reviewing', 'review_passed', 'escalated', 'revision_needed') THEN 'in_review'
                    WHEN prev_status IN ('pending_merge', 'merging') THEN 'merge'
                    WHEN prev_status IN ('approved', 'merged') THEN 'done'
                    ELSE NULL
                END AS column_id,
                (julianday(created_at) - julianday(prev_at)) * 24.0 * 60.0 AS dwell_minutes
            FROM transitions
            WHERE prev_at IS NOT NULL
              AND prev_status IS NOT NULL
        )
        SELECT
            column_id,
            AVG(dwell_minutes) AS avg_minutes,
            COUNT(*)           AS sample_size
        FROM column_mapped
        WHERE column_id IS NOT NULL
        GROUP BY column_id
        ORDER BY avg_minutes DESC
    ";

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| AppError::Database(e.to_string()))?;

    let column_names: &[(&str, &str)] = &[
        ("ready", "Ready"),
        ("in_progress", "In Progress"),
        ("in_review", "In Review"),
        ("merge", "Merge"),
        ("done", "Done"),
    ];

    let rows = stmt
        .query_map(params![project_id], |row| {
            let col_id: String = row.get(0)?;
            Ok((col_id, row.get::<_, f64>(1)?, row.get::<_, i64>(2)?))
        })
        .map_err(|e| AppError::Database(e.to_string()))?;

    let mut dwell_times = Vec::new();
    for row in rows {
        let (col_id, avg_minutes, sample_size) = row.map_err(|e| AppError::Database(e.to_string()))?;
        let col_name = column_names
            .iter()
            .find(|(id, _)| *id == col_id)
            .map(|(_, name)| name.to_string())
            .unwrap_or_else(|| col_id.clone());
        dwell_times.push(ColumnDwellTime {
            column_id: col_id,
            column_name: col_name,
            avg_minutes: (avg_minutes * 10.0).round() / 10.0,
            sample_size,
        });
    }
    Ok(dwell_times)
}

/// True average pipeline time: sum all non-terminal phase durations PER TASK,
/// then AVG across all tasks.  Returns `None` when no merged tasks exist.
pub(crate) fn query_avg_pipeline_time(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> AppResult<Option<f64>> {
    let sql = "
        WITH merged_tasks AS (
            SELECT id FROM tasks
            WHERE project_id = ?1
              AND internal_status = 'merged'
              AND datetime(updated_at) >= datetime('now', '-90 days')
        ),
        transitions AS (
            SELECT
                h.task_id,
                h.to_status,
                h.created_at,
                LAG(h.created_at) OVER (PARTITION BY h.task_id ORDER BY h.created_at) AS prev_at,
                LAG(h.to_status)  OVER (PARTITION BY h.task_id ORDER BY h.created_at) AS prev_status
            FROM task_state_history h
            WHERE h.task_id IN (SELECT id FROM merged_tasks)
        ),
        task_pipeline_minutes AS (
            SELECT
                tr.task_id,
                SUM((julianday(tr.created_at) - julianday(tr.prev_at)) * 24.0 * 60.0) AS pipeline_minutes
            FROM transitions tr
            WHERE tr.prev_at IS NOT NULL
              AND tr.prev_status NOT IN ('merged', 'cancelled', 'failed', 'stopped', 'paused', 'blocked')
            GROUP BY tr.task_id
        )
        SELECT AVG(pipeline_minutes) FROM task_pipeline_minutes
    ";

    let result: Option<f64> = conn
        .query_row(sql, params![project_id], |row| row.get(0))
        .map_err(|e| AppError::Database(e.to_string()))?;

    Ok(result)
}

// ─── Orchestrators ─────────────────────────────────────────────────────────────

/// Run all metric queries synchronously inside a single `db.run` closure.
/// `week_start_day`: 0=Sunday, 1=Monday, ..., 6=Saturday.
/// `tz_offset_minutes`: minutes east of UTC (e.g., AEST=+660, EST=-300).
pub fn compute_project_stats(
    conn: &rusqlite::Connection,
    project_id: &str,
    week_start_day: u8,
    tz_offset_minutes: i32,
) -> AppResult<ProjectStats> {
    let task_count = query_task_count(conn, project_id)?;
    let (today, this_week, this_month) =
        query_tasks_completed(conn, project_id, week_start_day, tz_offset_minutes)?;
    let (success_rate, success_count, total_count) = query_agent_success_rate(conn, project_id)?;
    let (pass_rate, pass_count, review_total) = query_review_pass_rate(conn, project_id)?;
    let cycle_time = query_cycle_time_breakdown(conn, project_id)?;
    let column_dwell = query_column_dwell_times(conn, project_id)?;
    let avg_pipeline = query_avg_pipeline_time(conn, project_id)?;
    let config = load_metrics_config(conn, project_id)?;
    let eme = query_eme(conn, project_id, &config)?;

    Ok(ProjectStats {
        task_count,
        tasks_completed_today: today,
        tasks_completed_this_week: this_week,
        tasks_completed_this_month: this_month,
        agent_success_rate: success_rate,
        agent_success_count: success_count,
        agent_total_count: total_count,
        review_pass_rate: pass_rate,
        review_pass_count: pass_count,
        review_total_count: review_total,
        cycle_time_breakdown: cycle_time,
        column_dwell_times: column_dwell,
        avg_pipeline_minutes: avg_pipeline,
        eme,
    })
}

// ─── Column / task metrics ────────────────────────────────────────────────────

/// Compute per-column task distribution using fixed Kanban column definitions.
pub fn compute_column_metrics(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> AppResult<Vec<ColumnMetric>> {
    let columns: &[(&str, &str, &[&str])] = &[
        ("backlog", "Backlog", &["backlog"]),
        ("ready", "Ready", &["ready", "revision_needed"]),
        (
            "in_progress",
            "In Progress",
            &["executing", "re_executing", "qa_refining", "qa_testing"],
        ),
        (
            "in_review",
            "In Review",
            &["pending_review", "reviewing", "review_passed"],
        ),
        (
            "done",
            "Done",
            &[
                "approved",
                "pending_merge",
                "merging",
                "merged",
                "failed",
                "cancelled",
                "stopped",
                "blocked",
            ],
        ),
    ];

    let mut result = Vec::with_capacity(columns.len());

    for (column_id, column_name, statuses) in columns {
        let placeholders: Vec<String> = (1..=statuses.len())
            .map(|i| format!("?{}", i + 1))
            .collect();
        let in_clause = placeholders.join(", ");

        let sql = format!(
            "SELECT
                COUNT(*) AS task_count,
                COALESCE(
                    AVG((julianday('now') - julianday(created_at)) * 24.0),
                    0.0
                ) AS avg_age_hours
             FROM tasks
             WHERE project_id = ?1
               AND archived_at IS NULL
               AND internal_status IN ({in_clause})"
        );

        let (task_count, avg_age_hours) = conn
            .query_row(
                &sql,
                rusqlite::params_from_iter(
                    std::iter::once(project_id.to_string())
                        .chain(statuses.iter().map(|s| s.to_string())),
                ),
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?)),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        result.push(ColumnMetric {
            column_id: column_id.to_string(),
            column_name: column_name.to_string(),
            task_count,
            avg_age_hours: (avg_age_hours * 10.0).round() / 10.0,
        });
    }

    Ok(result)
}

/// Compute per-task metrics from task_steps, reviews, and task_state_history.
pub fn compute_task_metrics(
    conn: &rusqlite::Connection,
    task_id: &str,
) -> AppResult<TaskMetrics> {
    let (step_count, completed_step_count) = conn
        .query_row(
            "SELECT
                COUNT(*) AS total,
                COUNT(CASE WHEN status = 'completed' THEN 1 END) AS done
             FROM task_steps
             WHERE task_id = ?1",
            params![task_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

    let (review_count, approved_review_count) = conn
        .query_row(
            "SELECT
                COUNT(*) AS total,
                COUNT(CASE WHEN status = 'approved' THEN 1 END) AS approved
             FROM reviews
             WHERE task_id = ?1",
            params![task_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

    let execution_minutes = {
        let sql = "
            WITH transitions AS (
                SELECT
                    to_status,
                    created_at,
                    LAG(created_at) OVER (ORDER BY created_at) AS prev_at,
                    LAG(to_status)  OVER (ORDER BY created_at) AS prev_status
                FROM task_state_history
                WHERE task_id = ?1
            )
            SELECT COALESCE(
                SUM((julianday(created_at) - julianday(prev_at)) * 24.0 * 60.0),
                0.0
            )
            FROM transitions
            WHERE prev_at IS NOT NULL
              AND prev_status IN ('executing', 're_executing')
        ";
        conn.query_row(sql, params![task_id], |row| row.get::<_, f64>(0))
            .map_err(|e| AppError::Database(e.to_string()))?
    };

    let total_age_hours = {
        let sql = "
            SELECT COALESCE(
                (julianday(
                    COALESCE(
                        (SELECT created_at FROM task_state_history
                         WHERE task_id = ?1 AND to_status = 'merged'
                         ORDER BY created_at DESC LIMIT 1),
                        'now'
                    )
                ) - julianday(created_at)) * 24.0,
                0.0
            )
            FROM tasks
            WHERE id = ?1
        ";
        conn.query_row(sql, params![task_id], |row| row.get::<_, f64>(0))
            .unwrap_or(0.0)
    };

    Ok(TaskMetrics {
        step_count,
        completed_step_count,
        review_count,
        approved_review_count,
        execution_minutes: (execution_minutes * 10.0).round() / 10.0,
        total_age_hours: (total_age_hours * 10.0).round() / 10.0,
    })
}
