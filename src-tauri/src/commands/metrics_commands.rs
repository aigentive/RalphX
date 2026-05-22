// Tauri commands: get_project_stats, get_project_trends, get_column_metrics, get_task_metrics
//
// Returns engineering metrics in a single response, with a 60s in-process cache
// keyed by project_id to avoid hitting SQLite on every popover open/close.
//
// Cache invalidation: call `invalidate_project_stats_cache` when a task changes state.
// The transition handler calls this on every state exit.
//
// Query logic is split across:
//   metrics_queries.rs  — stats queries + column/task metrics
//   metrics_trends.rs   — weekly trend queries

use std::sync::LazyLock;
use std::time::Instant;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;

pub use crate::commands::metrics_types::{
    ColumnDwellTime, ColumnMetric, CycleTimePhase, EmeEstimate, ProjectStats, ProjectTrends,
    TaskMetrics, WeeklyDataPoint,
};
pub use crate::commands::metrics_queries::{
    compute_column_metrics, compute_project_stats, compute_task_metrics,
};
pub use crate::commands::metrics_trends::compute_project_trends;

// ─── Caches ──────────────────────────────────────────────────────────────────

/// Global cache: project_id → (insertion instant, stats snapshot)
pub static STATS_CACHE: LazyLock<DashMap<String, (Instant, ProjectStats)>> =
    LazyLock::new(DashMap::new);

/// Global cache: project_id → (insertion instant, trends snapshot)
static TRENDS_CACHE: LazyLock<DashMap<String, (Instant, ProjectTrends)>> =
    LazyLock::new(DashMap::new);

/// Global cache: project_id → (insertion instant, column metrics snapshot)
pub static COLUMN_METRICS_CACHE: LazyLock<DashMap<String, (Instant, Vec<ColumnMetric>)>> =
    LazyLock::new(DashMap::new);

const CACHE_TTL_SECS: u64 = 60;

// ─── Config type ─────────────────────────────────────────────────────────────

/// Per-project EME calibration — base hours per tier and calendar factor.
/// Defaults are conservative. Users can override via the calibration UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsConfig {
    pub simple_base_hours: f64,
    pub medium_base_hours: f64,
    pub complex_base_hours: f64,
    pub calendar_factor: f64,
    pub working_days_per_week: i64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            simple_base_hours: 1.0,
            medium_base_hours: 2.0,
            complex_base_hours: 4.0,
            calendar_factor: 1.3,
            working_days_per_week: 5,
        }
    }
}

// ─── Cache helpers ────────────────────────────────────────────────────────────

/// Evict the cached stats, trends, and column metrics for `project_id`.
/// Called by the transition handler on every task state exit so the next
/// popover open always reflects the latest data.
/// Clears all week_start_day variants of the cache key.
pub fn invalidate_project_stats_cache(project_id: &str) {
    let prefix = format!("{}:", project_id);
    STATS_CACHE.retain(|k, _| !k.starts_with(&prefix));
    TRENDS_CACHE.retain(|k, _| !k.starts_with(&prefix));
    // Column metrics are not keyed by week_start_day
    COLUMN_METRICS_CACHE.remove(project_id);
}

// ─── Timezone validation ──────────────────────────────────────────────────────

/// Validate and clamp the timezone offset to the valid range UTC-12 to UTC+14.
fn validate_tz_offset(tz_offset_minutes: Option<i32>) -> Result<i32, String> {
    let tz = tz_offset_minutes.unwrap_or(0);
    if !(-720..=840).contains(&tz) {
        return Err(format!(
            "tz_offset_minutes must be between -720 and 840, got {}",
            tz
        ));
    }
    Ok(tz)
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

/// Return all 5 core metrics for a project in a single response.
///
/// Results are cached in memory for up to 60 seconds per project. The cache
/// is evicted whenever any task in the project changes state (via
/// `invalidate_project_stats_cache` called from the transition handler).
///
/// # Errors
/// Returns a string error if the database query fails.
#[tauri::command]
pub async fn get_project_stats(
    project_id: String,
    week_start_day: Option<u8>,
    tz_offset_minutes: Option<i32>,
    state: State<'_, AppState>,
) -> Result<ProjectStats, String> {
    let wsd = week_start_day.unwrap_or(0);
    let tz = validate_tz_offset(tz_offset_minutes)?;
    let cache_key = format!("{}:{}:{}", project_id, wsd, tz);

    if let Some(entry) = STATS_CACHE.get(&cache_key) {
        let (ts, stats) = &*entry;
        if ts.elapsed().as_secs() < CACHE_TTL_SECS {
            return Ok(stats.clone());
        }
    }

    let pid = project_id.clone();
    let stats = state
        .db
        .clone()
        .run(move |conn| compute_project_stats(conn, &pid, wsd, tz))
        .await
        .map_err(|e| e.to_string())?;

    STATS_CACHE.insert(cache_key, (Instant::now(), stats.clone()));
    Ok(stats)
}

/// Return time-series trend data for a project in a single response.
///
/// Results are cached in memory for up to 60 seconds per project. The cache
/// is evicted whenever any task in the project changes state.
///
/// # Errors
/// Returns a string error if the database query fails.
#[tauri::command]
pub async fn get_project_trends(
    project_id: String,
    week_start_day: Option<u8>,
    tz_offset_minutes: Option<i32>,
    state: State<'_, AppState>,
) -> Result<ProjectTrends, String> {
    let wsd = week_start_day.unwrap_or(0);
    let tz = validate_tz_offset(tz_offset_minutes)?;
    let cache_key = format!("{}:{}:{}", project_id, wsd, tz);

    if let Some(entry) = TRENDS_CACHE.get(&cache_key) {
        let (ts, trends) = &*entry;
        if ts.elapsed().as_secs() < CACHE_TTL_SECS {
            return Ok(trends.clone());
        }
    }

    let pid = project_id.clone();
    let trends = state
        .db
        .clone()
        .run(move |conn| compute_project_trends(conn, &pid, wsd, tz))
        .await
        .map_err(|e| e.to_string())?;

    TRENDS_CACHE.insert(cache_key, (Instant::now(), trends.clone()));
    Ok(trends)
}

/// Return the current EME calibration config for a project (or defaults).
///
/// # Errors
/// Returns a string error if the database query fails.
#[tauri::command]
pub async fn get_metrics_config(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<MetricsConfig, String> {
    let pid = project_id.clone();
    state
        .db
        .clone()
        .run(move |conn| load_metrics_config(conn, &pid))
        .await
        .map_err(|e| e.to_string())
}

/// Return per-column task distribution metrics for a project.
///
/// Results are cached for up to 60 seconds per project alongside `get_project_stats`.
/// The cache is invalidated whenever any task changes state.
///
/// # Errors
/// Returns a string error if the database query fails.
#[tauri::command]
pub async fn get_column_metrics(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ColumnMetric>, String> {
    if let Some(entry) = COLUMN_METRICS_CACHE.get(&project_id) {
        let (ts, metrics) = &*entry;
        if ts.elapsed().as_secs() < CACHE_TTL_SECS {
            return Ok(metrics.clone());
        }
    }

    let pid = project_id.clone();
    let metrics = state
        .db
        .clone()
        .run(move |conn| compute_column_metrics(conn, &pid))
        .await
        .map_err(|e| e.to_string())?;

    COLUMN_METRICS_CACHE.insert(project_id, (Instant::now(), metrics.clone()));
    Ok(metrics)
}

/// Return metrics for a single task.
///
/// Not cached — called on-demand from the task detail view.
///
/// # Errors
/// Returns a string error if the database query fails.
#[tauri::command]
pub async fn get_task_metrics(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<TaskMetrics, String> {
    state
        .db
        .clone()
        .run(move |conn| compute_task_metrics(conn, &task_id))
        .await
        .map_err(|e| e.to_string())
}

/// Save per-project EME calibration config and invalidate the stats cache.
///
/// # Errors
/// Returns a string error if the database query fails.
#[tauri::command]
pub async fn save_metrics_config(
    project_id: String,
    config: MetricsConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let pid = project_id.clone();
    state
        .db
        .clone()
        .run(move |conn| {
            conn.execute(
                "INSERT INTO project_metrics_config
                     (project_id, simple_base_hours, medium_base_hours, complex_base_hours, calendar_factor, working_days_per_week, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
                 ON CONFLICT(project_id) DO UPDATE SET
                     simple_base_hours    = excluded.simple_base_hours,
                     medium_base_hours    = excluded.medium_base_hours,
                     complex_base_hours   = excluded.complex_base_hours,
                     calendar_factor      = excluded.calendar_factor,
                     working_days_per_week = excluded.working_days_per_week,
                     updated_at           = excluded.updated_at",
                rusqlite::params![
                    pid,
                    config.simple_base_hours,
                    config.medium_base_hours,
                    config.complex_base_hours,
                    config.calendar_factor,
                    config.working_days_per_week
                ],
            )
            .map(|_| ())
            .map_err(|e| crate::error::AppError::Database(e.to_string()))
        })
        .await
        .map_err(|e| e.to_string())?;

    invalidate_project_stats_cache(&project_id);
    Ok(())
}

// ─── Config loader ───────────────────────────────────────────────────────────

/// Load per-project metrics config from DB, falling back to defaults if not set.
pub(crate) fn load_metrics_config(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> crate::error::AppResult<MetricsConfig> {
    let result = conn.query_row(
        "SELECT simple_base_hours, medium_base_hours, complex_base_hours, calendar_factor, working_days_per_week
         FROM project_metrics_config WHERE project_id = ?1",
        rusqlite::params![project_id],
        |row| {
            Ok(MetricsConfig {
                simple_base_hours: row.get(0)?,
                medium_base_hours: row.get(1)?,
                complex_base_hours: row.get(2)?,
                calendar_factor: row.get(3)?,
                working_days_per_week: row.get(4)?,
            })
        },
    );
    match result {
        Ok(config) => Ok(config),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(MetricsConfig::default()),
        Err(e) => Err(crate::error::AppError::Database(e.to_string())),
    }
}
