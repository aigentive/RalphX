// Migration v20260723130204: archive legacy project skill drafts

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "UPDATE project_skills
         SET status = 'archived',
             archived = 1,
             pinned = 0,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE status = 'staged'
           AND archived = 0
           AND json_valid(provenance_json)
           AND (
               json_extract(provenance_json, '$.additional.distiller') IN (
                   'deterministic_eligible_outcome_v1',
                   'git_history_commit_v1'
               )
               OR (
                   json_extract(provenance_json, '$.source') = 'github_pr_history'
                   AND json_extract(provenance_json, '$.authoring_contract') = 'project-skill-authoring'
               )
           )",
        [],
    )
    .map_err(|error| {
        AppError::Database(format!("failed to archive legacy project-skill drafts: {error}"))
    })?;
    Ok(())
}
