use async_trait::async_trait;
use rusqlite::OptionalExtension;

use crate::domain::entities::{ArtifactId, IdeationSessionId};
use crate::domain::repositories::{PlanArtifactApproval, PlanArtifactApprovalRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

pub struct SqlitePlanArtifactApprovalRepository {
    db: DbConnection,
}

impl SqlitePlanArtifactApprovalRepository {
    pub fn new(db: DbConnection) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PlanArtifactApprovalRepository for SqlitePlanArtifactApprovalRepository {
    async fn get_by_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<PlanArtifactApproval>> {
        let session_id_value = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT session_id, artifact_id, artifact_version,
                            blueprint_artifact_id, blueprint_artifact_version,
                            approved_at, approved_by
                     FROM plan_artifact_approvals
                     WHERE session_id = ?1 AND status = 'approved'",
                    [session_id_value],
                    |row| {
                        let session_id_raw: String = row.get(0)?;
                        let artifact_id_raw: String = row.get(1)?;
                        let version = row.get::<_, i64>(2)?;
                        let artifact_version = u32::try_from(version)
                            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, version))?;
                        Ok(PlanArtifactApproval {
                            session_id: IdeationSessionId::from_string(session_id_raw),
                            artifact_id: ArtifactId::from_string(artifact_id_raw),
                            artifact_version,
                            blueprint_artifact_id: row
                                .get::<_, Option<String>>(3)?
                                .map(ArtifactId::from_string),
                            blueprint_artifact_version: row
                                .get::<_, Option<i64>>(4)?
                                .map(|value| {
                                    u32::try_from(value).map_err(|_| {
                                        rusqlite::Error::IntegralValueOutOfRange(4, value)
                                    })
                                })
                                .transpose()?,
                            approved_at: row.get(5)?,
                            approved_by: row.get(6)?,
                        })
                    },
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn delete_by_session(&self, session_id: &IdeationSessionId) -> AppResult<usize> {
        let session_id_value = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM plan_artifact_approvals WHERE session_id = ?1",
                    [session_id_value],
                )
                .map_err(AppError::from)
            })
            .await
    }
}
