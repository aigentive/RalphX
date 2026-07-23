use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use super::sqlite_learned_skill_repos::{db_parse_error, parse_datetime};
use super::DbConnection;
use crate::domain::entities::{
    ProjectId, ProjectSkillEvidenceBatch, ProjectSkillEvidenceBatchId,
    ProjectSkillEvidenceBatchItem, ProjectSkillEvidenceBatchStatus, ProjectSkillId, TaskOutcomeId,
};
use crate::domain::repositories::ProjectSkillEvidenceBatchRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteProjectSkillEvidenceBatchRepository {
    db: DbConnection,
}

impl SqliteProjectSkillEvidenceBatchRepository {
    pub fn from_shared(connection: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(connection),
        }
    }
}

fn batch_from_connection(
    connection: &Connection,
    batch_id: &str,
) -> AppResult<Option<ProjectSkillEvidenceBatch>> {
    let mut batch = connection
        .query_row(
            "SELECT id, project_id, fingerprint, bucket, status, claim_token, claimed_at,
                    completed_project_skill_id, resolution_action, completed_at,
                    created_at, updated_at
             FROM project_skill_evidence_batches
             WHERE id = ?1",
            [batch_id],
            |row| {
                let status = row
                    .get::<_, String>("status")?
                    .parse::<ProjectSkillEvidenceBatchStatus>()
                    .map_err(db_parse_error)?;
                let claimed_at = row
                    .get::<_, Option<String>>("claimed_at")?
                    .map(|value| parse_datetime(&value));
                let completed_at = row
                    .get::<_, Option<String>>("completed_at")?
                    .map(|value| parse_datetime(&value));
                Ok(ProjectSkillEvidenceBatch {
                    id: ProjectSkillEvidenceBatchId::from_string(row.get::<_, String>("id")?),
                    project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
                    fingerprint: row.get("fingerprint")?,
                    bucket: row.get("bucket")?,
                    status,
                    claim_token: row.get("claim_token")?,
                    claimed_at,
                    completed_project_skill_id: row
                        .get::<_, Option<String>>("completed_project_skill_id")?
                        .map(ProjectSkillId::from_string),
                    resolution_action: row.get("resolution_action")?,
                    completed_at,
                    created_at: parse_datetime(&row.get::<_, String>("created_at")?),
                    updated_at: parse_datetime(&row.get::<_, String>("updated_at")?),
                    items: Vec::new(),
                })
            },
        )
        .optional()?;
    let Some(batch_row) = batch.as_mut() else {
        return Ok(None);
    };
    let mut statement = connection.prepare(
        "SELECT outcome_id, ordinal, digest
         FROM project_skill_evidence_batch_items
         WHERE batch_id = ?1
         ORDER BY ordinal ASC",
    )?;
    batch_row.items = statement
        .query_map([batch_id], |row| {
            Ok(ProjectSkillEvidenceBatchItem {
                outcome_id: TaskOutcomeId::from_string(row.get::<_, String>("outcome_id")?),
                ordinal: row.get::<_, i64>("ordinal")? as usize,
                digest: row.get("digest")?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(batch)
}

#[async_trait]
impl ProjectSkillEvidenceBatchRepository for SqliteProjectSkillEvidenceBatchRepository {
    async fn insert_if_absent(
        &self,
        batch: ProjectSkillEvidenceBatch,
    ) -> AppResult<ProjectSkillEvidenceBatch> {
        batch.validate_for_insert()?;
        self.db
            .run_transaction(move |connection| {
                let existing_id = connection
                    .query_row(
                        "SELECT id FROM project_skill_evidence_batches
                         WHERE project_id = ?1 AND fingerprint = ?2",
                        rusqlite::params![batch.project_id.as_str(), batch.fingerprint],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                if let Some(existing_id) = existing_id {
                    return batch_from_connection(connection, &existing_id)?.ok_or_else(|| {
                        AppError::Database(
                            "project skill evidence batch disappeared during insert".to_string(),
                        )
                    });
                }

                for item in &batch.items {
                    let outcome_project = connection
                        .query_row(
                            "SELECT project_id FROM task_outcomes WHERE id = ?1",
                            [item.outcome_id.as_str()],
                            |row| row.get::<_, String>(0),
                        )
                        .optional()?;
                    if outcome_project.as_deref() != Some(batch.project_id.as_str()) {
                        return Err(AppError::Validation(
                            "project skill evidence outcome belongs to a different project"
                                .to_string(),
                        ));
                    }
                }

                connection.execute(
                    "INSERT INTO project_skill_evidence_batches (
                        id, project_id, fingerprint, bucket, status, created_at, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6)",
                    rusqlite::params![
                        batch.id.as_str(),
                        batch.project_id.as_str(),
                        batch.fingerprint,
                        batch.bucket,
                        batch.created_at.to_rfc3339(),
                        batch.updated_at.to_rfc3339(),
                    ],
                )?;
                for item in &batch.items {
                    connection.execute(
                        "INSERT INTO project_skill_evidence_batch_items (
                            batch_id, outcome_id, ordinal, digest
                         ) VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![
                            batch.id.as_str(),
                            item.outcome_id.as_str(),
                            item.ordinal as i64,
                            item.digest,
                        ],
                    )?;
                }
                batch_from_connection(connection, batch.id.as_str())?.ok_or_else(|| {
                    AppError::Database(
                        "project skill evidence batch missing after insert".to_string(),
                    )
                })
            })
            .await
    }

    async fn list_batched_outcome_ids(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<TaskOutcomeId>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |connection| {
                let mut statement = connection.prepare(
                    "SELECT items.outcome_id
                     FROM project_skill_evidence_batch_items items
                     JOIN project_skill_evidence_batches batches
                       ON batches.id = items.batch_id
                     WHERE batches.project_id = ?1
                     ORDER BY items.outcome_id ASC",
                )?;
                let outcome_ids = statement
                    .query_map([project_id], |row| {
                        Ok(TaskOutcomeId::from_string(row.get::<_, String>(0)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(outcome_ids)
            })
            .await
    }

    async fn claim_oldest_pending(
        &self,
        project_id: &ProjectId,
        claim_token: &str,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>> {
        let project_id = project_id.as_str().to_string();
        let claim_token = claim_token.to_string();
        if claim_token.trim().is_empty() {
            return Err(AppError::Validation(
                "project skill evidence claim token is required".to_string(),
            ));
        }
        self.db
            .run_transaction(move |connection| {
                let batch_id = connection
                    .query_row(
                        "SELECT id FROM project_skill_evidence_batches
                         WHERE project_id = ?1 AND status = 'pending'
                         ORDER BY created_at ASC, id ASC
                         LIMIT 1",
                        [&project_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                let Some(batch_id) = batch_id else {
                    return Ok(None);
                };
                let changed = connection.execute(
                    "UPDATE project_skill_evidence_batches
                     SET status = 'consumed', claim_token = ?2, claimed_at = ?3, updated_at = ?3
                     WHERE id = ?1 AND status = 'pending'",
                    rusqlite::params![batch_id, claim_token, claimed_at.to_rfc3339()],
                )?;
                if changed != 1 {
                    return Ok(None);
                }
                batch_from_connection(connection, &batch_id)
            })
            .await
    }

    async fn release_claim(
        &self,
        batch_id: &ProjectSkillEvidenceBatchId,
        claim_token: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let batch_id = batch_id.as_str().to_string();
        let claim_token = claim_token.to_string();
        self.db
            .run_transaction(move |connection| {
                Ok(connection.execute(
                    "UPDATE project_skill_evidence_batches
                     SET status = 'pending', claim_token = NULL, claimed_at = NULL,
                         updated_at = ?3
                     WHERE id = ?1 AND status = 'consumed' AND claim_token = ?2
                       AND completed_at IS NULL",
                    rusqlite::params![batch_id, claim_token, updated_at.to_rfc3339()],
                )? == 1)
            })
            .await
    }

    async fn complete_claim(
        &self,
        batch_id: &ProjectSkillEvidenceBatchId,
        claim_token: &str,
        project_id: &ProjectId,
        project_skill_id: &ProjectSkillId,
        resolution_action: &str,
        completed_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let batch_id = batch_id.as_str().to_string();
        let claim_token = claim_token.to_string();
        let project_id = project_id.as_str().to_string();
        let project_skill_id = project_skill_id.as_str().to_string();
        let resolution_action = resolution_action.trim().to_string();
        if resolution_action.is_empty() {
            return Err(AppError::Validation(
                "project skill evidence resolution action is required".to_string(),
            ));
        }
        self.db
            .run_transaction(move |connection| {
                Ok(connection.execute(
                    "UPDATE project_skill_evidence_batches
                     SET completed_project_skill_id = ?4, resolution_action = ?5,
                         completed_at = ?6, updated_at = ?6
                     WHERE id = ?1 AND status = 'consumed' AND claim_token = ?2
                       AND project_id = ?3 AND completed_at IS NULL
                       AND EXISTS (
                           SELECT 1 FROM project_skills
                           WHERE id = ?4 AND project_id = ?3
                       )",
                    rusqlite::params![
                        batch_id,
                        claim_token,
                        project_id,
                        project_skill_id,
                        resolution_action,
                        completed_at.to_rfc3339(),
                    ],
                )? == 1)
            })
            .await
    }

    async fn requeue_stale_claims(
        &self,
        project_id: &ProjectId,
        stale_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<usize> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run_transaction(move |connection| {
                Ok(connection.execute(
                    "UPDATE project_skill_evidence_batches
                     SET status = 'pending', claim_token = NULL, claimed_at = NULL,
                         updated_at = ?3
                     WHERE project_id = ?1 AND status = 'consumed'
                       AND completed_at IS NULL AND claimed_at < ?2",
                    rusqlite::params![
                        project_id,
                        stale_before.to_rfc3339(),
                        updated_at.to_rfc3339(),
                    ],
                )?)
            })
            .await
    }

    async fn get_by_id(
        &self,
        batch_id: &ProjectSkillEvidenceBatchId,
    ) -> AppResult<Option<ProjectSkillEvidenceBatch>> {
        let batch_id = batch_id.as_str().to_string();
        self.db
            .run(move |connection| batch_from_connection(connection, &batch_id))
            .await
    }
}
