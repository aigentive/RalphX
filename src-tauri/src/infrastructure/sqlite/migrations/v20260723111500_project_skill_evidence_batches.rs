use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(connection: &Connection) -> AppResult<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS project_skill_evidence_batches (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                fingerprint TEXT NOT NULL CHECK (
                    length(fingerprint) = 64
                    AND fingerprint = lower(fingerprint)
                    AND fingerprint NOT GLOB '*[^0-9a-f]*'
                ),
                bucket TEXT NOT NULL CHECK (
                    bucket IN ('planning', 'verification', 'review', 'execution', 'merge')
                ),
                status TEXT NOT NULL CHECK (status IN ('pending', 'consumed', 'archived')),
                claim_token TEXT,
                claimed_at TEXT,
                completed_project_skill_id TEXT REFERENCES project_skills(id) ON DELETE SET NULL,
                resolution_action TEXT,
                completed_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(project_id, fingerprint),
                CHECK (
                    (
                        status = 'pending'
                        AND claim_token IS NULL
                        AND claimed_at IS NULL
                        AND completed_project_skill_id IS NULL
                        AND resolution_action IS NULL
                        AND completed_at IS NULL
                    )
                    OR (
                        status = 'consumed'
                        AND length(trim(claim_token)) > 0
                        AND claimed_at IS NOT NULL
                        AND (
                            (
                                completed_project_skill_id IS NULL
                                AND resolution_action IS NULL
                                AND completed_at IS NULL
                            )
                            OR (
                                completed_project_skill_id IS NOT NULL
                                AND length(trim(resolution_action)) > 0
                                AND completed_at IS NOT NULL
                            )
                        )
                    )
                    OR status = 'archived'
                )
             );
             CREATE TABLE IF NOT EXISTS project_skill_evidence_batch_items (
                batch_id TEXT NOT NULL
                    REFERENCES project_skill_evidence_batches(id) ON DELETE CASCADE,
                outcome_id TEXT NOT NULL REFERENCES task_outcomes(id) ON DELETE RESTRICT,
                ordinal INTEGER NOT NULL CHECK (ordinal >= 0 AND ordinal < 8),
                digest TEXT NOT NULL CHECK (
                    length(trim(digest)) > 0 AND length(digest) <= 1200
                ),
                PRIMARY KEY(batch_id, ordinal),
                UNIQUE(batch_id, outcome_id),
                UNIQUE(outcome_id)
             );
             CREATE INDEX IF NOT EXISTS idx_project_skill_evidence_batches_pending
                ON project_skill_evidence_batches(project_id, status, created_at, id);
             CREATE INDEX IF NOT EXISTS idx_project_skill_evidence_batches_claim
                ON project_skill_evidence_batches(id, claim_token, status);
             CREATE INDEX IF NOT EXISTS idx_project_skill_evidence_batches_stale
                ON project_skill_evidence_batches(project_id, status, claimed_at)
                WHERE completed_at IS NULL;",
        )
        .map_err(|error| AppError::Database(error.to_string()))
}
