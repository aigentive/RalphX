use crate::domain::entities::{
    IdeationSessionId, RemoteFinalizeDecision, RemoteFinalizeDecisionRequest,
    RemoteFinalizeDecisionRequestStatus,
};
use crate::domain::repositories::RemoteFinalizeDecisionRequestRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;

const COLUMNS: &str =
    "id,session_id,decision,status,error_code,result_json,claimed_at,created_at,updated_at";
pub struct SqliteRemoteFinalizeDecisionRequestRepository {
    db: DbConnection,
}
impl SqliteRemoteFinalizeDecisionRequestRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}
fn parse_time(value: &str, index: usize) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RemoteFinalizeDecisionRequest> {
    let claimed: Option<String> = r.get("claimed_at")?;
    let status = RemoteFinalizeDecisionRequestStatus::from_str(&r.get::<_, String>("status")?)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            )
        })?;
    let result: Option<String> = r.get("result_json")?;
    Ok(RemoteFinalizeDecisionRequest {
        id: r.get("id")?,
        session_id: IdeationSessionId::from_string(r.get::<_, String>("session_id")?),
        decision: RemoteFinalizeDecision::from_str(&r.get::<_, String>("decision")?).map_err(
            |error| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
                )
            },
        )?,
        status,
        error_code: r.get("error_code")?,
        result: result
            .map(|v| serde_json::from_str(&v))
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
        claimed_at: claimed.map(|v| parse_time(&v, 6)).transpose()?,
        created_at: parse_time(&r.get::<_, String>("created_at")?, 7)?,
        updated_at: parse_time(&r.get::<_, String>("updated_at")?, 8)?,
    })
}
#[async_trait]
impl RemoteFinalizeDecisionRequestRepository for SqliteRemoteFinalizeDecisionRequestRepository {
    async fn create_remote_finalize_decision_request(
        &self,
        r: RemoteFinalizeDecisionRequest,
    ) -> AppResult<RemoteFinalizeDecisionRequest> {
        let stored = r.clone();
        self.db.run(move|c|{c.execute("INSERT INTO remote_finalize_decision_requests(id,session_id,decision,status,error_code,result_json,claimed_at,created_at,updated_at)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![r.id,r.session_id.as_str(),r.decision.as_db_str(),r.status.as_db_str(),r.error_code,r.result.map(|v|v.to_string()),r.claimed_at.map(|v|v.to_rfc3339()),r.created_at.to_rfc3339(),r.updated_at.to_rfc3339()])?;Ok(())}).await?;
        Ok(stored)
    }
    async fn get(&self, id: &str) -> AppResult<Option<RemoteFinalizeDecisionRequest>> {
        let id = id.to_string();
        self.db
            .run(move |c| {
                c.query_row(
                    &format!("SELECT {COLUMNS} FROM remote_finalize_decision_requests WHERE id=?1"),
                    params![id],
                    row,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }
    async fn find_unsettled_for_session(
        &self,
        s: &IdeationSessionId,
    ) -> AppResult<Option<RemoteFinalizeDecisionRequest>> {
        let s = s.as_str().to_string();
        self.db.run(move|c|c.query_row(&format!("SELECT {COLUMNS} FROM remote_finalize_decision_requests WHERE session_id=?1 AND status IN('pending','starting') ORDER BY created_at,id LIMIT 1"),params![s],row).optional().map_err(AppError::from)).await
    }
    async fn claim_pending(
        &self,
        at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteFinalizeDecisionRequest>> {
        let at = at.to_rfc3339();
        self.db.run_transaction(move|c|{let id:Option<String>=c.query_row("SELECT id FROM remote_finalize_decision_requests WHERE status='pending' ORDER BY created_at,id LIMIT 1",[],|r|r.get(0)).optional()?;let Some(id)=id else{return Ok(None)};if c.execute("UPDATE remote_finalize_decision_requests SET status='starting',claimed_at=?1,updated_at=?1 WHERE id=?2 AND status='pending'",params![at,id])?==0{return Ok(None)};Ok(c.query_row(&format!("SELECT {COLUMNS} FROM remote_finalize_decision_requests WHERE id=?1"),params![id],row).optional()?)}).await
    }
    async fn complete(
        &self,
        id: &str,
        result: serde_json::Value,
        at: DateTime<Utc>,
    ) -> AppResult<()> {
        settle(&self.db, id, "completed", Some(result), None, at).await
    }
    async fn fail(&self, id: &str, code: &str, at: DateTime<Utc>) -> AppResult<()> {
        settle(&self.db, id, "failed", None, Some(code.to_string()), at).await
    }
    async fn fail_stale(&self, before: DateTime<Utc>, at: DateTime<Utc>) -> AppResult<u64> {
        let before = before.to_rfc3339();
        let at = at.to_rfc3339();
        self.db.run(move|c|Ok(c.execute("UPDATE remote_finalize_decision_requests SET status='failedStale',updated_at=?1 WHERE status='starting' AND claimed_at < ?2",params![at,before])? as u64)).await
    }
}
async fn settle(
    db: &DbConnection,
    id: &str,
    status: &str,
    result: Option<serde_json::Value>,
    error: Option<String>,
    at: DateTime<Utc>,
) -> AppResult<()> {
    let id = id.to_string();
    let status = status.to_string();
    let result = result.map(|v| v.to_string());
    let at = at.to_rfc3339();
    db.run(move|c|{c.execute("UPDATE remote_finalize_decision_requests SET status=?1,result_json=?2,error_code=?3,updated_at=?4 WHERE id=?5 AND status='starting'",params![status,result,error,at,id])?;Ok(())}).await
}
