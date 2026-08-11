use rusqlite::params_from_iter;

use crate::commands::metrics_scope::MetricsScope;
use crate::commands::metrics_types::{
    PrInsightItem, PrInsightOriginBreakdown, PrInsightsSummary, PrWeeklyThroughputPoint,
    ProjectPrInsights, WorkspaceStateDwellTime,
};
use crate::error::{AppError, AppResult};

fn pr_facts_cte(scope: MetricsScope<'_>) -> String {
    let workspace_filter = scope.project_filter("w");
    let plan_filter = scope.project_filter("pb");
    format!(
        "
    WITH workspace_event_bounds AS (
        SELECT
            conversation_id,
            MIN(CASE
                WHEN step IN ('pushed', 'published', 'external_pr_linked')
                  OR (step = 'publish' AND status = 'succeeded')
                THEN created_at
            END) AS first_published_at,
            MIN(CASE WHEN step IN ('pr_merged', 'external_pr_merged') THEN created_at END) AS pr_merged_at,
            MIN(CASE WHEN step IN ('pr_closed', 'external_pr_closed') THEN created_at END) AS pr_closed_at
        FROM agent_conversation_workspace_publication_events
        GROUP BY conversation_id
    ),
    workspace_prs AS (
        SELECT
            CASE
                WHEN w.linked_plan_branch_id IS NULL THEN 'agent_workspace_direct'
                ELSE 'agent_workspace_execution_owned'
            END AS origin,
            CASE
                WHEN w.linked_plan_branch_id IS NULL THEN 'Agent workspace'
                ELSE 'Execution-owned workspace'
            END AS label,
            CASE WHEN w.linked_plan_branch_id IS NULL THEN 1 ELSE 0 END AS counted_in_totals,
            CASE
                WHEN lower(COALESCE(w.publication_pr_status, '')) = 'merged' THEN 'merged'
                WHEN lower(COALESCE(w.publication_pr_status, '')) = 'closed' THEN 'closed'
                WHEN lower(COALESCE(w.publication_pr_status, '')) = 'draft' THEN 'draft'
                WHEN lower(COALESCE(w.publication_pr_status, '')) = 'changes_requested' THEN 'changes_requested'
                WHEN lower(COALESCE(w.publication_push_status, '')) IN ('pending', 'failed', 'description_failed') THEN 'unpushed'
                WHEN lower(COALESCE(w.publication_push_status, '')) = 'needs_agent' THEN 'needs_agent'
                ELSE 'open'
            END AS status,
            CASE
                WHEN lower(COALESCE(w.publication_push_status, '')) = 'needs_agent'
                  OR lower(COALESCE(w.pr_supervision_status, '')) IN ('fixing', 'blocked', 'publishing')
                  OR lower(COALESCE(w.publication_pr_status, '')) = 'changes_requested'
                THEN 1
                ELSE 0
            END AS needs_agent,
            CASE
                WHEN lower(COALESCE(w.publication_push_status, '')) IN ('pending', 'failed', 'description_failed')
                THEN 1
                ELSE 0
            END AS unpushed_workspace,
            w.publication_pr_number AS pr_number,
            w.publication_pr_url AS pr_url,
            w.branch_name AS branch_name,
            w.base_ref AS base_ref,
            w.conversation_id AS conversation_id,
            NULL AS task_id,
            w.linked_plan_branch_id AS plan_branch_id,
            COALESCE(b.first_published_at, w.created_at) AS opened_at,
            w.updated_at AS updated_at,
            COALESCE(
                b.pr_merged_at,
                CASE WHEN lower(COALESCE(w.publication_pr_status, '')) = 'merged' THEN w.updated_at END
            ) AS merged_at,
            COALESCE(
                b.pr_closed_at,
                CASE WHEN lower(COALESCE(w.publication_pr_status, '')) = 'closed' THEN w.updated_at END
            ) AS closed_at
        FROM agent_conversation_workspaces w
        LEFT JOIN workspace_event_bounds b ON b.conversation_id = w.conversation_id
        WHERE {workspace_filter}
          AND w.publication_pr_number IS NOT NULL
    ),
    plan_prs AS (
        SELECT
            'task_pipeline_pr_mode' AS origin,
            'Task pipeline PR mode' AS label,
            1 AS counted_in_totals,
            CASE
                WHEN lower(COALESCE(pb.pr_status, '')) = 'merged' THEN 'merged'
                WHEN lower(COALESCE(pb.pr_status, '')) = 'closed' THEN 'closed'
                WHEN lower(COALESCE(pb.pr_status, '')) = 'draft' THEN 'draft'
                WHEN lower(COALESCE(pb.pr_status, '')) = 'open' THEN 'open'
                WHEN pb.pr_draft = 1 THEN 'draft'
                ELSE 'open'
            END AS status,
            0 AS needs_agent,
            0 AS unpushed_workspace,
            pb.pr_number AS pr_number,
            pb.pr_url AS pr_url,
            pb.branch_name AS branch_name,
            COALESCE(pb.base_branch_override, pb.source_branch) AS base_ref,
            NULL AS conversation_id,
            pb.merge_task_id AS task_id,
            pb.id AS plan_branch_id,
            pb.created_at AS opened_at,
            pb.last_polled_at AS updated_at,
            pb.merged_at AS merged_at,
            CASE WHEN lower(COALESCE(pb.pr_status, '')) = 'closed' THEN COALESCE(pb.last_polled_at, pb.merged_at) END AS closed_at
        FROM plan_branches pb
        WHERE {plan_filter}
          AND pb.pr_number IS NOT NULL
    ),
    all_prs AS (
        SELECT * FROM workspace_prs
        UNION ALL
        SELECT * FROM plan_prs
    )
"
    )
}

/// Compute PR and agent-workspace performance metrics for a project.
pub fn compute_project_pr_insights(
    conn: &rusqlite::Connection,
    project_id: &str,
    week_start_day: u8,
    tz_offset_minutes: i32,
) -> AppResult<ProjectPrInsights> {
    compute_pr_insights_for_scope(
        conn,
        MetricsScope::Project(project_id),
        week_start_day,
        tz_offset_minutes,
    )
}

pub fn compute_insights_pr_insights(
    conn: &rusqlite::Connection,
    week_start_day: u8,
    tz_offset_minutes: i32,
) -> AppResult<ProjectPrInsights> {
    compute_pr_insights_for_scope(
        conn,
        MetricsScope::AllProjects,
        week_start_day,
        tz_offset_minutes,
    )
}

fn compute_pr_insights_for_scope(
    conn: &rusqlite::Connection,
    scope: MetricsScope<'_>,
    week_start_day: u8,
    tz_offset_minutes: i32,
) -> AppResult<ProjectPrInsights> {
    let origins = query_origin_breakdowns(conn, scope)?;
    let workspace_summary = query_workspace_summary(conn, scope)?;
    let event_summary = query_publication_event_summary(conn, scope)?;
    let avg_workspace_pr_cycle_hours = query_avg_workspace_pr_cycle_hours(conn, scope)?;
    let avg_plan_pr_wait_hours = query_avg_plan_pr_wait_hours(conn, scope)?;
    let weekly_throughput =
        query_weekly_pr_throughput(conn, scope, week_start_day, tz_offset_minutes)?;
    let workspace_dwell_times = query_workspace_state_dwell_times(conn, scope)?;
    let latest_prs = query_latest_pr_items(conn, scope)?;

    let counted_origins = origins.iter().filter(|origin| origin.counted_in_totals);
    let mut summary = counted_origins.fold(
        PrInsightsSummary {
            total_prs: 0,
            direct_workspace_prs: 0,
            task_pipeline_prs: 0,
            execution_owned_workspace_refs: 0,
            merged_prs: 0,
            open_prs: 0,
            draft_prs: 0,
            changes_requested_prs: 0,
            closed_prs: 0,
            needs_agent_prs: 0,
            unpushed_workspace_prs: 0,
            total_workspaces: workspace_summary.total_workspaces,
            direct_workspaces: workspace_summary.direct_workspaces,
            direct_workspaces_with_prs: workspace_summary.direct_workspaces_with_prs,
            direct_workspace_pr_conversion_rate: ratio(
                workspace_summary.direct_workspaces_with_prs,
                workspace_summary.direct_workspaces,
            ),
            terminal_merge_rate: 0.0,
            avg_workspace_pr_cycle_hours,
            avg_plan_pr_wait_hours,
            requested_changes_events: event_summary.requested_changes_events,
            autofix_needed_events: event_summary.autofix_needed_events,
            agent_fix_completed_events: event_summary.agent_fix_completed_events,
            supervision_enabled_workspaces: workspace_summary.supervision_enabled_workspaces,
            auto_merge_desired_workspaces: workspace_summary.auto_merge_desired_workspaces,
            auto_merge_active_workspaces: workspace_summary.auto_merge_active_workspaces,
        },
        |mut acc, origin| {
            acc.total_prs += origin.total_prs;
            acc.merged_prs += origin.merged_prs;
            acc.open_prs += origin.open_prs;
            acc.draft_prs += origin.draft_prs;
            acc.changes_requested_prs += origin.changes_requested_prs;
            acc.closed_prs += origin.closed_prs;
            acc.needs_agent_prs += origin.needs_agent_prs;
            acc.unpushed_workspace_prs += origin.unpushed_workspace_prs;
            match origin.origin.as_str() {
                "agent_workspace_direct" => acc.direct_workspace_prs += origin.total_prs,
                "task_pipeline_pr_mode" => acc.task_pipeline_prs += origin.total_prs,
                _ => {}
            }
            acc
        },
    );

    summary.execution_owned_workspace_refs = origins
        .iter()
        .find(|origin| origin.origin == "agent_workspace_execution_owned")
        .map(|origin| origin.total_prs)
        .unwrap_or(0);
    summary.terminal_merge_rate =
        ratio(summary.merged_prs, summary.merged_prs + summary.closed_prs);

    Ok(ProjectPrInsights {
        summary,
        origins,
        weekly_throughput,
        workspace_dwell_times,
        latest_prs,
    })
}

fn query_origin_breakdowns(
    conn: &rusqlite::Connection,
    scope: MetricsScope<'_>,
) -> AppResult<Vec<PrInsightOriginBreakdown>> {
    let pr_facts_cte = pr_facts_cte(scope);
    let sql = format!(
        "{pr_facts_cte}
        SELECT
            origin,
            label,
            counted_in_totals,
            COUNT(*) AS total_prs,
            SUM(CASE WHEN status = 'merged' THEN 1 ELSE 0 END) AS merged_prs,
            SUM(CASE WHEN status = 'open' THEN 1 ELSE 0 END) AS open_prs,
            SUM(CASE WHEN status = 'draft' THEN 1 ELSE 0 END) AS draft_prs,
            SUM(CASE WHEN status = 'changes_requested' THEN 1 ELSE 0 END) AS changes_requested_prs,
            SUM(CASE WHEN status = 'closed' THEN 1 ELSE 0 END) AS closed_prs,
            SUM(CASE WHEN needs_agent = 1 THEN 1 ELSE 0 END) AS needs_agent_prs,
            SUM(CASE WHEN unpushed_workspace = 1 THEN 1 ELSE 0 END) AS unpushed_workspace_prs
        FROM all_prs
        GROUP BY origin, label, counted_in_totals
        ORDER BY
            CASE origin
                WHEN 'agent_workspace_direct' THEN 0
                WHEN 'task_pipeline_pr_mode' THEN 1
                ELSE 2
            END"
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|error| AppError::Database(error.to_string()))?;
    let rows = stmt
        .query_map(params_from_iter(scope.project_params()), |row| {
            Ok(PrInsightOriginBreakdown {
                origin: row.get(0)?,
                label: row.get(1)?,
                counted_in_totals: row.get::<_, i64>(2)? != 0,
                total_prs: row.get(3)?,
                merged_prs: row.get(4)?,
                open_prs: row.get(5)?,
                draft_prs: row.get(6)?,
                changes_requested_prs: row.get(7)?,
                closed_prs: row.get(8)?,
                needs_agent_prs: row.get(9)?,
                unpushed_workspace_prs: row.get(10)?,
            })
        })
        .map_err(|error| AppError::Database(error.to_string()))?;

    collect_rows(rows)
}

fn query_weekly_pr_throughput(
    conn: &rusqlite::Connection,
    scope: MetricsScope<'_>,
    week_start_day: u8,
    tz_offset_minutes: i32,
) -> AppResult<Vec<PrWeeklyThroughputPoint>> {
    let wt = (week_start_day + 6) % 7;
    let tz_off = format!("{:+} minutes", tz_offset_minutes);
    let pr_facts_cte = pr_facts_cte(scope);
    let sql = format!(
        "{pr_facts_cte}
        , throughput_events AS (
            SELECT
                date(opened_at, '{tz_off}', 'weekday {wt}', '-6 days') AS week_start,
                1 AS opened,
                0 AS merged
            FROM all_prs
            WHERE counted_in_totals = 1
              AND opened_at IS NOT NULL
            UNION ALL
            SELECT
                date(merged_at, '{tz_off}', 'weekday {wt}', '-6 days') AS week_start,
                0 AS opened,
                1 AS merged
            FROM all_prs
            WHERE counted_in_totals = 1
              AND merged_at IS NOT NULL
        ),
        grouped AS (
            SELECT
                week_start,
                SUM(opened) AS opened,
                SUM(merged) AS merged,
                SUM(opened + merged) AS sample_size
            FROM throughput_events
            WHERE week_start IS NOT NULL
            GROUP BY week_start
            ORDER BY week_start DESC
            LIMIT 12
        )
        SELECT week_start, opened, merged, sample_size
        FROM grouped
        ORDER BY week_start ASC"
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|error| AppError::Database(error.to_string()))?;
    let rows = stmt
        .query_map(params_from_iter(scope.project_params()), |row| {
            Ok(PrWeeklyThroughputPoint {
                week_start: row.get(0)?,
                opened: row.get(1)?,
                merged: row.get(2)?,
                sample_size: row.get(3)?,
            })
        })
        .map_err(|error| AppError::Database(error.to_string()))?;

    collect_rows(rows)
}

fn query_latest_pr_items(
    conn: &rusqlite::Connection,
    scope: MetricsScope<'_>,
) -> AppResult<Vec<PrInsightItem>> {
    let pr_facts_cte = pr_facts_cte(scope);
    let sql = format!(
        "{pr_facts_cte}
        SELECT
            origin,
            label,
            counted_in_totals,
            status,
            pr_number,
            pr_url,
            branch_name,
            base_ref,
            conversation_id,
            task_id,
            plan_branch_id,
            opened_at,
            updated_at,
            merged_at
        FROM all_prs
        ORDER BY datetime(COALESCE(updated_at, merged_at, opened_at)) DESC
        LIMIT 8"
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|error| AppError::Database(error.to_string()))?;
    let rows = stmt
        .query_map(params_from_iter(scope.project_params()), |row| {
            Ok(PrInsightItem {
                origin: row.get(0)?,
                label: row.get(1)?,
                counted_in_totals: row.get::<_, i64>(2)? != 0,
                status: row.get(3)?,
                pr_number: row.get(4)?,
                pr_url: row.get(5)?,
                branch_name: row.get(6)?,
                base_ref: row.get(7)?,
                conversation_id: row.get(8)?,
                task_id: row.get(9)?,
                plan_branch_id: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
                merged_at: row.get(13)?,
            })
        })
        .map_err(|error| AppError::Database(error.to_string()))?;

    collect_rows(rows)
}

fn query_avg_workspace_pr_cycle_hours(
    conn: &rusqlite::Connection,
    scope: MetricsScope<'_>,
) -> AppResult<Option<f64>> {
    let pr_facts_cte = pr_facts_cte(scope);
    let sql = format!(
        "{pr_facts_cte}
        SELECT AVG((julianday(COALESCE(merged_at, closed_at)) - julianday(opened_at)) * 24.0)
        FROM all_prs
        WHERE origin = 'agent_workspace_direct'
          AND status IN ('merged', 'closed')
          AND opened_at IS NOT NULL
          AND COALESCE(merged_at, closed_at) IS NOT NULL"
    );
    optional_f64(conn, &sql, scope)
}

fn query_avg_plan_pr_wait_hours(
    conn: &rusqlite::Connection,
    scope: MetricsScope<'_>,
) -> AppResult<Option<f64>> {
    let plan_filter = scope.project_filter("pb");
    let sql = format!(
        "
        WITH pr_merge_tasks AS (
            SELECT pb.merge_task_id AS task_id
            FROM plan_branches pb
            WHERE {plan_filter}
              AND pr_number IS NOT NULL
              AND merge_task_id IS NOT NULL
        ),
        transitions AS (
            SELECT
                h.task_id,
                h.to_status,
                h.created_at,
                LEAD(h.created_at) OVER (PARTITION BY h.task_id ORDER BY h.created_at) AS next_at
            FROM task_state_history h
            WHERE h.task_id IN (SELECT task_id FROM pr_merge_tasks)
        )
        SELECT AVG((julianday(next_at) - julianday(created_at)) * 24.0)
        FROM transitions
        WHERE to_status = 'waiting_on_pr'
          AND next_at IS NOT NULL
    "
    );
    optional_f64(conn, &sql, scope)
}

fn query_workspace_state_dwell_times(
    conn: &rusqlite::Connection,
    scope: MetricsScope<'_>,
) -> AppResult<Vec<WorkspaceStateDwellTime>> {
    let workspace_filter = scope.project_filter("w");
    let sql = format!(
        "
        WITH ordered AS (
            SELECT
                h.conversation_id,
                h.state_family,
                h.to_state,
                h.created_at,
                LAG(h.to_state) OVER (
                    PARTITION BY h.conversation_id, h.state_family
                    ORDER BY datetime(h.created_at), h.id
                ) AS prev_state
            FROM agent_conversation_workspace_state_history h
            JOIN agent_conversation_workspaces w ON w.conversation_id = h.conversation_id
            WHERE {workspace_filter}
              AND datetime(h.created_at) >= datetime('now', '-365 days')
        ),
        change_points AS (
            SELECT conversation_id, state_family, to_state, created_at
            FROM ordered
            WHERE prev_state IS NULL
               OR prev_state != to_state
        ),
        spans AS (
            SELECT
                conversation_id,
                state_family,
                to_state,
                created_at,
                LEAD(created_at) OVER (
                    PARTITION BY conversation_id, state_family
                    ORDER BY datetime(created_at)
                ) AS next_at
            FROM change_points
        )
        SELECT
            state_family,
            to_state,
            AVG((julianday(next_at) - julianday(created_at)) * 24.0 * 60.0) AS avg_minutes,
            COUNT(*) AS sample_size
        FROM spans
        WHERE next_at IS NOT NULL
          AND to_state != 'none'
          AND julianday(next_at) > julianday(created_at)
        GROUP BY state_family, to_state
        ORDER BY avg_minutes DESC
        LIMIT 12
    "
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|error| AppError::Database(error.to_string()))?;
    let rows = stmt
        .query_map(params_from_iter(scope.project_params()), |row| {
            let state_family: String = row.get(0)?;
            let state: String = row.get(1)?;
            let avg_minutes: f64 = row.get(2)?;
            Ok(WorkspaceStateDwellTime {
                label: workspace_state_label(&state_family, &state),
                state_family,
                state,
                avg_minutes: (avg_minutes * 10.0).round() / 10.0,
                sample_size: row.get(3)?,
            })
        })
        .map_err(|error| AppError::Database(error.to_string()))?;

    collect_rows(rows)
}

#[derive(Debug)]
struct WorkspaceSummary {
    total_workspaces: i64,
    direct_workspaces: i64,
    direct_workspaces_with_prs: i64,
    supervision_enabled_workspaces: i64,
    auto_merge_desired_workspaces: i64,
    auto_merge_active_workspaces: i64,
}

fn query_workspace_summary(
    conn: &rusqlite::Connection,
    scope: MetricsScope<'_>,
) -> AppResult<WorkspaceSummary> {
    let workspace_filter = scope.project_filter("w");
    let sql = format!(
        "
        SELECT
            COUNT(*) AS total_workspaces,
            COALESCE(SUM(CASE WHEN linked_plan_branch_id IS NULL THEN 1 ELSE 0 END), 0) AS direct_workspaces,
            COALESCE(SUM(CASE WHEN linked_plan_branch_id IS NULL AND publication_pr_number IS NOT NULL THEN 1 ELSE 0 END), 0) AS direct_workspaces_with_prs,
            COALESCE(SUM(CASE WHEN publication_pr_number IS NOT NULL AND pr_autofix_enabled = 1 THEN 1 ELSE 0 END), 0) AS supervision_enabled_workspaces,
            COALESCE(SUM(CASE WHEN publication_pr_number IS NOT NULL AND pr_auto_merge_desired = 1 THEN 1 ELSE 0 END), 0) AS auto_merge_desired_workspaces,
            COALESCE(SUM(CASE WHEN publication_pr_number IS NOT NULL AND pr_auto_merge_current = 1 THEN 1 ELSE 0 END), 0) AS auto_merge_active_workspaces
        FROM agent_conversation_workspaces w
        WHERE {workspace_filter}
          AND status != 'archived'
    "
    );
    conn.query_row(&sql, params_from_iter(scope.project_params()), |row| {
        Ok(WorkspaceSummary {
            total_workspaces: row.get(0)?,
            direct_workspaces: row.get(1)?,
            direct_workspaces_with_prs: row.get(2)?,
            supervision_enabled_workspaces: row.get(3)?,
            auto_merge_desired_workspaces: row.get(4)?,
            auto_merge_active_workspaces: row.get(5)?,
        })
    })
    .map_err(|error| AppError::Database(error.to_string()))
}

#[derive(Debug)]
struct PublicationEventSummary {
    requested_changes_events: i64,
    autofix_needed_events: i64,
    agent_fix_completed_events: i64,
}

fn query_publication_event_summary(
    conn: &rusqlite::Connection,
    scope: MetricsScope<'_>,
) -> AppResult<PublicationEventSummary> {
    let workspace_filter = scope.project_filter("w");
    let sql = format!(
        "
        SELECT
            COALESCE(SUM(CASE
                WHEN e.step = 'github_review'
                  OR e.classification LIKE 'github_pr_review:%'
                THEN 1 ELSE 0
            END), 0) AS requested_changes_events,
            COALESCE(SUM(CASE
                WHEN e.step = 'pr_autofix' AND e.status = 'needs_agent'
                THEN 1 ELSE 0
            END), 0) AS autofix_needed_events,
            COALESCE(SUM(CASE
                WHEN e.step IN ('pr_autofix_completed', 'repair_completed')
                  OR (e.step = 'pr_autofix' AND e.status = 'succeeded')
                THEN 1 ELSE 0
            END), 0) AS agent_fix_completed_events
        FROM agent_conversation_workspace_publication_events e
        JOIN agent_conversation_workspaces w ON w.conversation_id = e.conversation_id
        WHERE {workspace_filter}
    "
    );
    conn.query_row(&sql, params_from_iter(scope.project_params()), |row| {
        Ok(PublicationEventSummary {
            requested_changes_events: row.get(0)?,
            autofix_needed_events: row.get(1)?,
            agent_fix_completed_events: row.get(2)?,
        })
    })
    .map_err(|error| AppError::Database(error.to_string()))
}

fn optional_f64(
    conn: &rusqlite::Connection,
    sql: &str,
    scope: MetricsScope<'_>,
) -> AppResult<Option<f64>> {
    conn.query_row(sql, params_from_iter(scope.project_params()), |row| {
        row.get(0)
    })
    .map_err(|error| AppError::Database(error.to_string()))
}

fn ratio(numerator: i64, denominator: i64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn workspace_state_label(state_family: &str, state: &str) -> String {
    let state_label = match state {
        "active" => "Active",
        "archived" => "Archived",
        "missing" => "Missing",
        "open" => "PR Open",
        "draft" => "Draft PR",
        "merged" => "Merged",
        "closed" => "Closed",
        "pushed" => "Pushed",
        "refreshed" => "Refreshed",
        "needs_agent" => "Needs Agent",
        "failed" => "Failed",
        "description_failed" => "Description Failed",
        "monitoring" => "Monitoring",
        "fixing" => "Fixing",
        "blocked" => "Blocked",
        "disabled" => "Disabled",
        "no_changes" => "No Changes",
        _ => state,
    };
    let family_label = match state_family {
        "workspace_status" => "Workspace",
        "publication_pr_status" => "PR",
        "publication_push_status" => "Publication",
        "pr_supervision_status" => "Supervision",
        _ => state_family,
    };
    format!("{family_label}: {state_label}")
}

fn collect_rows<T, F>(rows: rusqlite::MappedRows<'_, F>) -> AppResult<Vec<T>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let mut values = Vec::new();
    for row in rows {
        values.push(row.map_err(|error| AppError::Database(error.to_string()))?);
    }
    Ok(values)
}
