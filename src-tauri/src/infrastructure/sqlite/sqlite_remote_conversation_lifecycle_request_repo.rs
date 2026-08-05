use crate::domain::{
    entities::{
        RemoteConversationLifecycleKind, RemoteConversationLifecycleRequest,
        RemoteConversationLifecycleStatus,
    },
    repositories::RemoteConversationLifecycleRequestRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;
const C:&str="id,kind,conversation_id,close_pull_request,allocated_conversation_id,status,error_code,result_json,claimed_at,created_at,updated_at";
pub struct SqliteRemoteConversationLifecycleRequestRepository {
    db: DbConnection,
}
impl SqliteRemoteConversationLifecycleRequestRepository {
    pub fn from_shared(c: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(c),
        }
    }
}
fn dt(v: &str, i: usize) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, Box::new(e))
        })
}
fn inv(i: usize, e: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        i,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
    )
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RemoteConversationLifecycleRequest> {
    let result: Option<String> = r.get("result_json")?;
    let claimed: Option<String> = r.get("claimed_at")?;
    Ok(RemoteConversationLifecycleRequest {
        id: r.get("id")?,
        kind: RemoteConversationLifecycleKind::from_str(&r.get::<_, String>("kind")?)
            .map_err(|e| inv(1, e))?,
        conversation_id: r.get("conversation_id")?,
        close_pull_request: r.get("close_pull_request")?,
        allocated_conversation_id: r.get("allocated_conversation_id")?,
        status: RemoteConversationLifecycleStatus::from_str(&r.get::<_, String>("status")?)
            .map_err(|e| inv(5, e))?,
        error_code: r.get("error_code")?,
        result: result
            .map(|v| serde_json::from_str(&v))
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
        claimed_at: claimed.map(|v| dt(&v, 8)).transpose()?,
        created_at: dt(&r.get::<_, String>("created_at")?, 9)?,
        updated_at: dt(&r.get::<_, String>("updated_at")?, 10)?,
    })
}
#[async_trait]
impl RemoteConversationLifecycleRequestRepository
    for SqliteRemoteConversationLifecycleRequestRepository
{
    async fn create_remote_conversation_lifecycle_request(
        &self,
        r: RemoteConversationLifecycleRequest,
    ) -> AppResult<RemoteConversationLifecycleRequest> {
        let out = r.clone();
        self.db.run(move|c|{c.execute("INSERT INTO remote_conversation_lifecycle_requests(id,kind,conversation_id,close_pull_request,allocated_conversation_id,status,error_code,result_json,claimed_at,created_at,updated_at)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![r.id,r.kind.as_db_str(),r.conversation_id,r.close_pull_request,r.allocated_conversation_id,r.status.as_db_str(),r.error_code,r.result.map(|v|v.to_string()),r.claimed_at.map(|v|v.to_rfc3339()),r.created_at.to_rfc3339(),r.updated_at.to_rfc3339()])?;Ok(())}).await?;
        Ok(out)
    }
    async fn get(&self, id: &str) -> AppResult<Option<RemoteConversationLifecycleRequest>> {
        let id = id.to_string();
        self.db
            .run(move |c| {
                c.query_row(
                    &format!("SELECT {C} FROM remote_conversation_lifecycle_requests WHERE id=?1"),
                    [id],
                    row,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }
    async fn find_unsettled(
        &self,
        cid: &str,
    ) -> AppResult<Option<RemoteConversationLifecycleRequest>> {
        let cid = cid.to_string();
        self.db.run(move|c|c.query_row(&format!("SELECT {C} FROM remote_conversation_lifecycle_requests WHERE conversation_id=?1 AND status IN('pending','starting') ORDER BY created_at,id LIMIT 1"),[cid],row).optional().map_err(AppError::from)).await
    }
    async fn claim_pending(
        &self,
        at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteConversationLifecycleRequest>> {
        let at = at.to_rfc3339();
        self.db.run_transaction(move|c|{let id:Option<String>=c.query_row("SELECT id FROM remote_conversation_lifecycle_requests WHERE status='pending' ORDER BY created_at,id LIMIT 1",[],|r|r.get(0)).optional()?;let Some(id)=id else{return Ok(None)};if c.execute("UPDATE remote_conversation_lifecycle_requests SET status='starting',claimed_at=?1,updated_at=?1 WHERE id=?2 AND status='pending'",params![at,id])?==0{return Ok(None)}Ok(c.query_row(&format!("SELECT {C} FROM remote_conversation_lifecycle_requests WHERE id=?1"),[id],row).optional()?) }).await
    }
    async fn complete(
        &self,
        id: &str,
        result: serde_json::Value,
        at: DateTime<Utc>,
    ) -> AppResult<()> {
        settle(&self.db, id, "completed", None, Some(result), at).await
    }
    async fn fail(&self, id: &str, code: &str, at: DateTime<Utc>) -> AppResult<()> {
        settle(&self.db, id, "failed", Some(code.into()), None, at).await
    }
    async fn fail_stale(&self, before: DateTime<Utc>, at: DateTime<Utc>) -> AppResult<u64> {
        let b = before.to_rfc3339();
        let a = at.to_rfc3339();
        self.db.run(move|c|Ok(c.execute("UPDATE remote_conversation_lifecycle_requests SET status='failedStale',updated_at=?1 WHERE status='starting' AND claimed_at < ?2",params![a,b])? as u64)).await
    }
}
async fn settle(
    db: &DbConnection,
    id: &str,
    status: &str,
    error: Option<String>,
    result: Option<serde_json::Value>,
    at: DateTime<Utc>,
) -> AppResult<()> {
    let id = id.to_string();
    let status = status.to_string();
    let result = result.map(|v| v.to_string());
    let at = at.to_rfc3339();
    db.run(move|c|{c.execute("UPDATE remote_conversation_lifecycle_requests SET status=?1,error_code=?2,result_json=?3,updated_at=?4 WHERE id=?5 AND status='starting'",params![status,error,result,at,id])?;Ok(())}).await
}
