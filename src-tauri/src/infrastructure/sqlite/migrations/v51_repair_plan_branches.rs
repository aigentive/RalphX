// Migration v51: Repair plan_branches schema + backfill execution_plans
//
// This migration handles TWO scenarios:
//
// **Fresh DB** (v46-v50 all ran correctly):
//   - plan_branches has UNIQUE on plan_artifact_id (from v13) + execution_plan_id column (from v47)
//   - execution_plans table exists (from v46), v49 already backfilled
//   - Need to: drop UNIQUE on plan_artifact_id (multiple sessions can share same artifact)
//   - Backfill is a no-op (v49 already handled it)
//
// **Dev DB** (our old v49_fix_ghost_plan_branches ran):
//   - plan_branches has 10 columns (NO execution_plan_id, NO UNIQUE on plan_artifact_id)
//   - execution_plans table was DROPPED by old v49
//   - tasks.execution_plan_id exists (from ghost v48)
//   - v49 backfill never ran (DB was already at version 49)
//   - Need to: recreate execution_plans, add execution_plan_id, AND backfill
//
// Strategy:
//   1. CREATE TABLE IF NOT EXISTS execution_plans (safe for both scenarios)
//   2. Recreate plan_branches with 11 columns: NO UNIQUE on plan_artifact_id + execution_plan_id
//   3. Use column_exists to determine INSERT strategy (10-col vs 11-col source)
//   4. add_column_if_not_exists for project_active_plan + tasks
//   5. Create indexes IF NOT EXISTS
//   6. Backfill: create ExecutionPlan records for accepted/archived sessions missing them

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    // Step 1: Ensure execution_plans table exists (dropped by old v49 on dev DB)
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS execution_plans (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES ideation_sessions(id),
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );",
    )
    .map_err(|e| AppError::Database(format!("v51: failed to ensure execution_plans: {}", e)))?;

    // Step 2: Recreate plan_branches with correct schema
    conn.execute("PRAGMA foreign_keys = OFF", [])
        .map_err(|e| AppError::Database(e.to_string()))?;

    // Check if source plan_branches has execution_plan_id column
    let has_execution_plan_id = helpers::column_exists(conn, "plan_branches", "execution_plan_id");

    conn.execute_batch(
        "-- Safety: drop partial leftover from interrupted previous run
        DROP TABLE IF EXISTS plan_branches_new;

        -- Recreate plan_branches with 11 columns:
        --   - NO UNIQUE on plan_artifact_id (multiple sessions can share same artifact)
        --   - YES execution_plan_id (links to execution_plans table)
        CREATE TABLE plan_branches_new (
            id TEXT PRIMARY KEY,
            plan_artifact_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            branch_name TEXT NOT NULL,
            source_branch TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            merge_task_id TEXT,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            merged_at TEXT,
            execution_plan_id TEXT REFERENCES execution_plans(id)
        );",
    )
    .map_err(|e| AppError::Database(format!("v51: failed to create plan_branches_new: {}", e)))?;

    // Copy data — strategy depends on whether source has execution_plan_id
    if has_execution_plan_id {
        // Fresh DB (v47 added the column): copy all 11 columns
        conn.execute_batch(
            "INSERT INTO plan_branches_new
                (id, plan_artifact_id, session_id, project_id, branch_name, source_branch,
                 status, merge_task_id, created_at, merged_at, execution_plan_id)
            SELECT id, plan_artifact_id, session_id, project_id, branch_name, source_branch,
                   status, merge_task_id, created_at, merged_at, execution_plan_id
            FROM plan_branches;",
        )
        .map_err(|e| {
            AppError::Database(format!("v51: failed to copy plan_branches (11-col): {}", e))
        })?;
    } else {
        // Dev DB (old v49 removed the column): copy 10 columns, execution_plan_id defaults to NULL
        conn.execute_batch(
            "INSERT INTO plan_branches_new
                (id, plan_artifact_id, session_id, project_id, branch_name, source_branch,
                 status, merge_task_id, created_at, merged_at)
            SELECT id, plan_artifact_id, session_id, project_id, branch_name, source_branch,
                   status, merge_task_id, created_at, merged_at
            FROM plan_branches;",
        )
        .map_err(|e| {
            AppError::Database(format!("v51: failed to copy plan_branches (10-col): {}", e))
        })?;
    }

    conn.execute_batch(
        "-- Drop old table (also drops all inline constraints and indexes)
        DROP TABLE plan_branches;

        -- Rename new table
        ALTER TABLE plan_branches_new RENAME TO plan_branches;

        -- Recreate indexes
        -- UNIQUE on session_id (from v16)
        CREATE UNIQUE INDEX idx_plan_branches_session_id
            ON plan_branches(session_id);

        -- Non-unique index on plan_artifact_id for lookup performance
        CREATE INDEX idx_plan_branches_plan_artifact_id
            ON plan_branches(plan_artifact_id);

        -- Unique index on execution_plan_id where not null (from v47)
        CREATE UNIQUE INDEX IF NOT EXISTS idx_plan_branches_execution_plan
            ON plan_branches(execution_plan_id) WHERE execution_plan_id IS NOT NULL;",
    )
    .map_err(|e| AppError::Database(format!("v51: failed to finalize plan_branches: {}", e)))?;

    conn.execute("PRAGMA foreign_keys = ON", [])
        .map_err(|e| AppError::Database(e.to_string()))?;

    // Step 3: Ensure execution_plan_id column exists on related tables
    helpers::add_column_if_not_exists(
        conn,
        "project_active_plan",
        "execution_plan_id",
        "TEXT REFERENCES execution_plans(id)",
    )?;

    helpers::add_column_if_not_exists(
        conn,
        "tasks",
        "execution_plan_id",
        "TEXT REFERENCES execution_plans(id)",
    )?;

    // Step 4: Ensure tasks index exists
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_tasks_execution_plan
         ON tasks(execution_plan_id);",
    )
    .map_err(|e| AppError::Database(format!("v51: failed to create tasks index: {}", e)))?;

    // Step 5: Backfill execution_plans for accepted/archived sessions that have none.
    // On fresh DB this is a no-op (v49 already backfilled). On dev DB, v49 was skipped
    // (DB was already at version 49 from old v49_fix_ghost_plan_branches), so we must
    // create ExecutionPlan records and link tasks/branches/active_plan here.
    backfill_execution_plans(conn)?;

    Ok(())
}

/// Create ExecutionPlan records for accepted/archived sessions missing them,
/// then link their tasks, plan_branches, and project_active_plan entries.
/// Idempotent: skips sessions that already have an ExecutionPlan.
fn backfill_execution_plans(conn: &Connection) -> AppResult<()> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id FROM ideation_sessions s
             LEFT JOIN execution_plans ep ON ep.session_id = s.id
             WHERE s.status IN ('accepted', 'archived') AND ep.id IS NULL",
        )
        .map_err(|e| AppError::Database(format!("v51 backfill: failed to prepare query: {}", e)))?;

    let session_ids: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| AppError::Database(format!("v51 backfill: failed to query sessions: {}", e)))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            AppError::Database(format!("v51 backfill: failed to collect sessions: {}", e))
        })?;

    drop(stmt);

    tracing::info!(
        "v51 backfill: found {} session(s) needing ExecutionPlan",
        session_ids.len()
    );

    for session_id in &session_ids {
        let execution_plan_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        // Create the execution_plan record
        conn.execute(
            "INSERT INTO execution_plans (id, session_id, status, created_at)
             VALUES (?1, ?2, 'active', ?3)",
            rusqlite::params![execution_plan_id, session_id, now],
        )
        .map_err(|e| {
            AppError::Database(format!(
                "v51 backfill: failed to insert execution_plan for session {}: {}",
                session_id, e
            ))
        })?;

        // Link tasks belonging to this session with no execution_plan_id
        conn.execute(
            "UPDATE tasks SET execution_plan_id = ?1
             WHERE ideation_session_id = ?2 AND execution_plan_id IS NULL",
            rusqlite::params![execution_plan_id, session_id],
        )
        .map_err(|e| {
            AppError::Database(format!(
                "v51 backfill: failed to link tasks for session {}: {}",
                session_id, e
            ))
        })?;

        // Link plan_branches belonging to this session with no execution_plan_id
        conn.execute(
            "UPDATE plan_branches SET execution_plan_id = ?1
             WHERE session_id = ?2 AND execution_plan_id IS NULL",
            rusqlite::params![execution_plan_id, session_id],
        )
        .map_err(|e| {
            AppError::Database(format!(
                "v51 backfill: failed to link plan_branches for session {}: {}",
                session_id, e
            ))
        })?;

        // Populate execution_plan_id on project_active_plan entries for this session
        conn.execute(
            "UPDATE project_active_plan SET execution_plan_id = ?1
             WHERE ideation_session_id = ?2 AND execution_plan_id IS NULL",
            rusqlite::params![execution_plan_id, session_id],
        )
        .map_err(|e| {
            AppError::Database(format!(
                "v51 backfill: failed to update active_plan for session {}: {}",
                session_id, e
            ))
        })?;
    }

    tracing::info!(
        "v51 backfill: created {} execution_plan(s) and linked existing data",
        session_ids.len()
    );

    Ok(())
}
