// Response types shared across metrics commands.
// Extracted from metrics_commands.rs to keep that file under the 500-line limit.

use serde::Serialize;

/// Average time spent in each pipeline phase, derived from LAG() window function.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CycleTimePhase {
    /// Internal status label (e.g. "ready", "executing", "pending_review")
    pub phase: String,
    /// Average minutes spent in this phase across sampled tasks
    pub avg_minutes: f64,
    /// Number of task-transitions that contributed to this average
    pub sample_size: i64,
}

/// Estimated Manual Effort range (low..high hours).
/// Only populated when ≥5 tasks are completed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmeEstimate {
    pub low_hours: f64,
    pub high_hours: f64,
    /// Scope covered by this estimate. Direct agent workspaces are not included.
    pub scope: String,
    pub scope_label: String,
    /// Number of merged tasks used in the estimate
    pub task_count: i64,
    /// ISO date of the earliest merged task in the sample
    pub earliest_task_date: Option<String>,
    /// ISO date of the most recent merged task in the sample
    pub latest_task_date: Option<String>,
}

/// All project metrics returned by the `get_project_stats` command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStats {
    // ── Throughput ──────────────────────────────────────────────────────────
    /// Total non-archived tasks in the project (used by frontend threshold logic)
    pub task_count: i64,
    pub tasks_completed_today: i64,
    pub tasks_completed_this_week: i64,
    pub tasks_completed_this_month: i64,

    // ── Quality ─────────────────────────────────────────────────────────────
    /// merged / (merged + failed + cancelled + stopped), 0.0 when denominator is 0
    pub agent_success_rate: f64,
    pub agent_success_count: i64,
    pub agent_total_count: i64,

    /// approved / (approved + changes_requested), 0.0 when denominator is 0
    pub review_pass_rate: f64,
    pub review_pass_count: i64,
    pub review_total_count: i64,

    // ── Cycle time ──────────────────────────────────────────────────────────
    /// Per-phase averages over the last 90 days (merged tasks only)
    pub cycle_time_breakdown: Vec<CycleTimePhase>,

    // ── Column dwell time ──────────────────────────────────────────────────
    /// Average time tasks spend in each Kanban column (last 90 days, merged tasks only)
    pub column_dwell_times: Vec<ColumnDwellTime>,

    // ── Average pipeline time ────────────────────────────────────────────────
    /// True per-task average pipeline time in minutes (sum phases per task, then avg).
    /// `None` when no merged tasks in the last 90 days.
    pub avg_pipeline_minutes: Option<f64>,

    // ── EME (Estimated Manual Effort) ────────────────────────────────────────
    /// None when < 5 merged tasks exist (insufficient sample)
    pub eme: Option<EmeEstimate>,
}

/// A single weekly data point for trend charts.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyDataPoint {
    /// ISO date string "YYYY-MM-DD" representing the start of the week (Sunday)
    pub week_start: String,
    /// The metric value for this week
    pub value: f64,
    /// Number of tasks/data points that contributed to this value
    pub sample_size: i64,
}

/// Time-series trend data returned by the `get_project_trends` command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTrends {
    /// Count of tasks merged per week, last 12 weeks
    pub weekly_throughput: Vec<WeeklyDataPoint>,
    /// Deduped delivery throughput for task completions plus direct agent workspace PR output.
    pub weekly_delivery_throughput: Vec<DeliveryWeeklyThroughputPoint>,
    /// Average cycle time in hours for merged tasks per week, last 12 weeks
    pub weekly_cycle_time: Vec<WeeklyDataPoint>,
    /// Average pipeline cycle time (all non-terminal phases) in hours per week, last 12 weeks
    pub weekly_pipeline_cycle_time: Vec<WeeklyDataPoint>,
    /// Percentage of merged vs total terminal tasks per week, last 12 weeks
    pub weekly_success_rate: Vec<WeeklyDataPoint>,
}

/// A weekly delivery throughput point split by task pipeline and direct agent workspaces.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryWeeklyThroughputPoint {
    pub week_start: String,
    pub unified_deliveries: i64,
    pub task_deliveries: i64,
    pub workspace_deliveries: i64,
    pub merged_prs: i64,
    pub sample_size: i64,
}

/// Pull-request and agent-workspace performance metrics for Insights.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPrInsights {
    pub summary: PrInsightsSummary,
    pub origins: Vec<PrInsightOriginBreakdown>,
    pub weekly_throughput: Vec<PrWeeklyThroughputPoint>,
    pub workspace_dwell_times: Vec<WorkspaceStateDwellTime>,
    pub latest_prs: Vec<PrInsightItem>,
}

/// Roll-up counters for PR velocity, outcomes, rework, and supervision.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrInsightsSummary {
    pub total_prs: i64,
    pub direct_workspace_prs: i64,
    pub task_pipeline_prs: i64,
    pub execution_owned_workspace_refs: i64,
    pub merged_prs: i64,
    pub open_prs: i64,
    pub draft_prs: i64,
    pub changes_requested_prs: i64,
    pub closed_prs: i64,
    pub needs_agent_prs: i64,
    pub unpushed_workspace_prs: i64,
    pub total_workspaces: i64,
    pub direct_workspaces: i64,
    pub direct_workspaces_with_prs: i64,
    pub direct_workspace_pr_conversion_rate: f64,
    pub terminal_merge_rate: f64,
    pub avg_workspace_pr_cycle_hours: Option<f64>,
    pub avg_plan_pr_wait_hours: Option<f64>,
    pub requested_changes_events: i64,
    pub autofix_needed_events: i64,
    pub agent_fix_completed_events: i64,
    pub supervision_enabled_workspaces: i64,
    pub auto_merge_desired_workspaces: i64,
    pub auto_merge_active_workspaces: i64,
}

/// PR counts for one source family.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrInsightOriginBreakdown {
    pub origin: String,
    pub label: String,
    pub counted_in_totals: bool,
    pub total_prs: i64,
    pub merged_prs: i64,
    pub open_prs: i64,
    pub draft_prs: i64,
    pub changes_requested_prs: i64,
    pub closed_prs: i64,
    pub needs_agent_prs: i64,
    pub unpushed_workspace_prs: i64,
}

/// Weekly PR throughput for the last 12 weeks.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrWeeklyThroughputPoint {
    pub week_start: String,
    pub opened: i64,
    pub merged: i64,
    pub sample_size: i64,
}

/// Average time spent in an agent workspace/publication state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStateDwellTime {
    pub state_family: String,
    pub state: String,
    pub label: String,
    pub avg_minutes: f64,
    pub sample_size: i64,
}

/// Latest PR/workspace facts for audit-oriented Insights surfaces.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrInsightItem {
    pub origin: String,
    pub label: String,
    pub counted_in_totals: bool,
    pub status: String,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub branch_name: String,
    pub base_ref: String,
    pub conversation_id: Option<String>,
    pub task_id: Option<String>,
    pub plan_branch_id: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub merged_at: Option<String>,
}

/// Average dwell time per Kanban column, derived from task_state_history transitions.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDwellTime {
    /// Kanban column id (e.g. "ready", "in_progress", "in_review", "merge", "done")
    pub column_id: String,
    /// Human-readable column name
    pub column_name: String,
    /// Average minutes tasks spent in this column
    pub avg_minutes: f64,
    /// Number of task-transitions that contributed to this average
    pub sample_size: i64,
}

/// Per-column task distribution metric for the Kanban board.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMetric {
    /// Kanban column id (e.g. "backlog", "ready", "in_progress", "in_review", "done")
    pub column_id: String,
    /// Human-readable column name
    pub column_name: String,
    /// Number of non-archived tasks currently in this column
    pub task_count: i64,
    /// Average age of tasks in this column in hours (0 when task_count is 0)
    pub avg_age_hours: f64,
}

/// Per-task metrics returned by `get_task_metrics`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskMetrics {
    /// Total steps (all statuses)
    pub step_count: i64,
    /// Steps with status = 'completed'
    pub completed_step_count: i64,
    /// Number of review cycles for this task
    pub review_count: i64,
    /// Approved reviews
    pub approved_review_count: i64,
    /// Time spent in 'executing' or 're_executing' phases, in minutes (0 when no history)
    pub execution_minutes: f64,
    /// Total elapsed time from task creation to now (or merge), in hours
    pub total_age_hours: f64,
}
