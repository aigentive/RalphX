// Migration v20260523145711: plan complexity assessments

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS plan_complexity_assessments (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES ideation_sessions(id) ON DELETE CASCADE,
            artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
            artifact_version INTEGER NOT NULL,
            level TEXT NOT NULL CHECK(level IN (
                'trivial', 'simple', 'moderate', 'complex', 'very_complex'
            )),
            score INTEGER NOT NULL CHECK(score >= 0 AND score <= 100),
            recommended_action TEXT NOT NULL CHECK(recommended_action IN (
                'implement_directly', 'create_proposals'
            )),
            confidence REAL NOT NULL CHECK(confidence >= 0.0 AND confidence <= 1.0),
            reason_summary TEXT NOT NULL,
            signals_json TEXT NOT NULL DEFAULT '{}',
            assessed_by TEXT NOT NULL DEFAULT 'ralphx-utility-plan-complexity',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(session_id, artifact_id, artifact_version)
        );

        CREATE INDEX IF NOT EXISTS idx_plan_complexity_assessments_session
            ON plan_complexity_assessments(session_id, updated_at DESC);

        CREATE INDEX IF NOT EXISTS idx_plan_complexity_assessments_artifact
            ON plan_complexity_assessments(artifact_id, artifact_version);",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    Ok(())
}
