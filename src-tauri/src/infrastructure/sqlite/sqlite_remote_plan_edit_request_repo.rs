use crate::{
    domain::{
        entities::{RemotePlanEditRequest, RemotePlanEditRequestStatus},
        repositories::RemotePlanEditRequestRepository,
    },
    error::{AppError, AppResult},
    infrastructure::sqlite::DbConnection,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;
const COLUMNS:&str="id,artifact_id,content,expected_version,status,error_code,result_json,claimed_at,created_at,updated_at";
pub struct SqliteRemotePlanEditRequestRepository {
    db: DbConnection,
}
impl SqliteRemotePlanEditRequestRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}
fn time(v: &str, i: usize) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, Box::new(e))
        })
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RemotePlanEditRequest> {
    let claimed: Option<String> = r.get("claimed_at")?;
    let result: Option<String> = r.get("result_json")?;
    Ok(RemotePlanEditRequest {
        id: r.get("id")?,
        artifact_id: r.get("artifact_id")?,
        content: r.get("content")?,
        expected_version: r.get("expected_version")?,
        status: RemotePlanEditRequestStatus::from_str(&r.get::<_, String>("status")?).map_err(
            |e| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                )
            },
        )?,
        error_code: r.get("error_code")?,
        result: result
            .map(|v| serde_json::from_str(&v))
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
        claimed_at: claimed.map(|v| time(&v, 7)).transpose()?,
        created_at: time(&r.get::<_, String>("created_at")?, 8)?,
        updated_at: time(&r.get::<_, String>("updated_at")?, 9)?,
    })
}
#[async_trait]
impl RemotePlanEditRequestRepository for SqliteRemotePlanEditRequestRepository {
    async fn create_remote_plan_edit_request(
        &self,
        r: RemotePlanEditRequest,
    ) -> AppResult<RemotePlanEditRequest> {
        let saved = r.clone();
        self.db.run(move|c|{c.execute("INSERT INTO remote_plan_edit_requests(id,artifact_id,content,expected_version,status,error_code,result_json,claimed_at,created_at,updated_at)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![r.id,r.artifact_id,r.content,r.expected_version,r.status.as_db_str(),r.error_code,r.result.map(|v|v.to_string()),r.claimed_at.map(|v|v.to_rfc3339()),r.created_at.to_rfc3339(),r.updated_at.to_rfc3339()])?;Ok(())}).await?;
        Ok(saved)
    }
    async fn get(&self, id: &str) -> AppResult<Option<RemotePlanEditRequest>> {
        let id = id.to_string();
        self.db
            .run(move |c| {
                c.query_row(
                    &format!("SELECT {COLUMNS} FROM remote_plan_edit_requests WHERE id=?1"),
                    params![id],
                    row,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }
    async fn find_unsettled_for_artifact(
        &self,
        artifact_id: &str,
    ) -> AppResult<Option<RemotePlanEditRequest>> {
        let id = artifact_id.to_string();
        self.db.run(move|c|c.query_row(&format!("SELECT {COLUMNS} FROM remote_plan_edit_requests WHERE artifact_id=?1 AND status IN('pending','starting') ORDER BY created_at,id LIMIT 1"),params![id],row).optional().map_err(AppError::from)).await
    }
    async fn claim_pending(&self, at: DateTime<Utc>) -> AppResult<Option<RemotePlanEditRequest>> {
        let at = at.to_rfc3339();
        self.db.run_transaction(move|c|{let id:Option<String>=c.query_row("SELECT id FROM remote_plan_edit_requests WHERE status='pending' ORDER BY created_at,id LIMIT 1",[],|r|r.get(0)).optional()?;let Some(id)=id else{return Ok(None)};if c.execute("UPDATE remote_plan_edit_requests SET status='starting',claimed_at=?1,updated_at=?1 WHERE id=?2 AND status='pending'",params![at,id])?==0{return Ok(None)};Ok(c.query_row(&format!("SELECT {COLUMNS} FROM remote_plan_edit_requests WHERE id=?1"),params![id],row).optional()?)}).await
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
        self.db.run(move|c|Ok(c.execute("UPDATE remote_plan_edit_requests SET status='failedStale',updated_at=?1 WHERE status='starting' AND claimed_at < ?2",params![at,before])? as u64)).await
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
    let (id, status, result, at) = (
        id.to_string(),
        status.to_string(),
        result.map(|v| v.to_string()),
        at.to_rfc3339(),
    );
    db.run(move|c|{c.execute("UPDATE remote_plan_edit_requests SET status=?1,result_json=?2,error_code=?3,updated_at=?4 WHERE id=?5 AND status='starting'",params![status,result,error,at,id])?;Ok(())}).await
}
