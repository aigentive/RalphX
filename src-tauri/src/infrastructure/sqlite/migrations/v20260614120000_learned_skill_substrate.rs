use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_outcomes (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            source TEXT NOT NULL,
            source_ref_kind TEXT NOT NULL,
            source_ref_id TEXT NOT NULL,
            task_id TEXT,
            conversation_id TEXT,
            agent_run_id TEXT,
            pull_request_id TEXT,
            proposal_id TEXT,
            verification_id TEXT,
            review_id TEXT,
            outcome_class TEXT,
            status TEXT NOT NULL CHECK (status IN (
                'unknown', 'eligible', 'ineligible', 'succeeded', 'failed'
            )),
            evidence_json TEXT NOT NULL DEFAULT '{}',
            provider_harness TEXT,
            provider_session_id TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            CHECK (length(source) > 0),
            CHECK (length(source_ref_kind) > 0),
            CHECK (length(source_ref_id) > 0)
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_task_outcomes_source_ref
            ON task_outcomes(project_id, source, source_ref_kind, source_ref_id);

        CREATE INDEX IF NOT EXISTS idx_task_outcomes_project_status
            ON task_outcomes(project_id, status, updated_at DESC);

        CREATE TABLE IF NOT EXISTS project_skills (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            bucket TEXT NOT NULL,
            stage TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN (
                'staged', 'approved', 'rejected', 'archived', 'retired'
            )),
            pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
            archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
            scope_paths_json TEXT NOT NULL DEFAULT '[]',
            compact_guidance TEXT NOT NULL,
            body_markdown TEXT NOT NULL,
            predicted_effect TEXT,
            provenance_json TEXT NOT NULL DEFAULT '{}',
            companion_of_skill_id TEXT REFERENCES project_skills(id) ON DELETE SET NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            CHECK (length(title) > 0),
            CHECK (length(bucket) > 0),
            CHECK (length(stage) > 0),
            CHECK (status != 'staged' OR predicted_effect IS NOT NULL)
        );

        CREATE INDEX IF NOT EXISTS idx_project_skills_project_status
            ON project_skills(project_id, status, archived, updated_at DESC);

        CREATE INDEX IF NOT EXISTS idx_project_skills_project_stage_bucket
            ON project_skills(project_id, stage, bucket, status);

        CREATE TABLE IF NOT EXISTS skill_usage_events (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            project_skill_id TEXT NOT NULL REFERENCES project_skills(id) ON DELETE CASCADE,
            conversation_id TEXT,
            agent_run_id TEXT,
            provider_harness TEXT,
            stage TEXT,
            bucket TEXT,
            injection_kind TEXT NOT NULL,
            outcome_id TEXT REFERENCES task_outcomes(id) ON DELETE SET NULL,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            CHECK (length(injection_kind) > 0)
        );

        CREATE INDEX IF NOT EXISTS idx_skill_usage_events_project_created
            ON skill_usage_events(project_id, created_at DESC);

        CREATE INDEX IF NOT EXISTS idx_skill_usage_events_skill_created
            ON skill_usage_events(project_skill_id, created_at DESC);

        CREATE INDEX IF NOT EXISTS idx_skill_usage_events_run
            ON skill_usage_events(agent_run_id)
            WHERE agent_run_id IS NOT NULL;",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    Ok(())
}
