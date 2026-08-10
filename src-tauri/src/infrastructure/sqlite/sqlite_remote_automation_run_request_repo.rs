use crate::domain::{
    entities::{
        RemoteAutomationRunKind, RemoteAutomationRunRequest, RemoteAutomationRunRequestStatus,
    },
    repositories::RemoteAutomationRunRequestRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;
const COLUMNS:&str="id,automation_id,kind,expected_run_id,status,error_code,result_json,claimed_at,created_at,updated_at";
pub struct SqliteRemoteAutomationRunRequestRepository {
    db: DbConnection,
}
impl SqliteRemoteAutomationRunRequestRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}
fn time(value: &str, index: usize) -> rusqlite::Result<DateTime<Utc>> {
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
fn invalid(index: usize, error: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
    )
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RemoteAutomationRunRequest> {
    let result: Option<String> = r.get("result_json")?;
    let claimed: Option<String> = r.get("claimed_at")?;
    Ok(RemoteAutomationRunRequest {
        id: r.get("id")?,
        automation_id: r.get("automation_id")?,
        kind: RemoteAutomationRunKind::from_str(&r.get::<_, String>("kind")?)
            .map_err(|e| invalid(2, e))?,
        expected_run_id: r.get("expected_run_id")?,
        status: RemoteAutomationRunRequestStatus::from_str(&r.get::<_, String>("status")?)
            .map_err(|e| invalid(4, e))?,
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
impl RemoteAutomationRunRequestRepository for SqliteRemoteAutomationRunRequestRepository {
    async fn create_remote_automation_run_request(
        &self,
        r: RemoteAutomationRunRequest,
    ) -> AppResult<RemoteAutomationRunRequest> {
        let stored = r.clone();
        self.db.run(move|c|{c.execute("INSERT INTO remote_automation_run_requests(id,automation_id,kind,expected_run_id,status,error_code,result_json,claimed_at,created_at,updated_at)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![r.id,r.automation_id,r.kind.as_db_str(),r.expected_run_id,r.status.as_db_str(),r.error_code,r.result.map(|v|v.to_string()),r.claimed_at.map(|v|v.to_rfc3339()),r.created_at.to_rfc3339(),r.updated_at.to_rfc3339()])?;Ok(())}).await?;
        Ok(stored)
    }
    async fn get(&self, id: &str) -> AppResult<Option<RemoteAutomationRunRequest>> {
        let id = id.to_string();
        self.db
            .run(move |c| {
                c.query_row(
                    &format!("SELECT {COLUMNS} FROM remote_automation_run_requests WHERE id=?1"),
                    params![id],
                    row,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }
    async fn find_unsettled(
        &self,
        automation_id: &str,
        kind: RemoteAutomationRunKind,
    ) -> AppResult<Option<RemoteAutomationRunRequest>> {
        let id = automation_id.to_string();
        let kind = kind.as_db_str().to_string();
        self.db.run(move|c|c.query_row(&format!("SELECT {COLUMNS} FROM remote_automation_run_requests WHERE automation_id=?1 AND kind=?2 AND status IN('pending','starting') ORDER BY created_at,id LIMIT 1"),params![id,kind],row).optional().map_err(AppError::from)).await
    }
    async fn claim_pending(
        &self,
        at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteAutomationRunRequest>> {
        let at = at.to_rfc3339();
        self.db.run_transaction(move|c|{let id:Option<String>=c.query_row("SELECT id FROM remote_automation_run_requests WHERE status='pending' ORDER BY created_at,id LIMIT 1",[],|r|r.get(0)).optional()?;let Some(id)=id else{return Ok(None)};if c.execute("UPDATE remote_automation_run_requests SET status='starting',claimed_at=?1,updated_at=?1 WHERE id=?2 AND status='pending'",params![at,id])?==0{return Ok(None)};Ok(c.query_row(&format!("SELECT {COLUMNS} FROM remote_automation_run_requests WHERE id=?1"),params![id],row).optional()?) }).await
    }
    async fn complete(
        &self,
        id: &str,
        result: serde_json::Value,
        at: DateTime<Utc>,
    ) -> AppResult<()> {
        settle(&self.db, id, "completed", Some(result), None, at).await
    }
    async fn fail(
        &self,
        id: &str,
        code: &str,
        result: Option<serde_json::Value>,
        at: DateTime<Utc>,
    ) -> AppResult<()> {
        settle(&self.db, id, "failed", result, Some(code.to_string()), at).await
    }
    async fn fail_stale(&self, before: DateTime<Utc>, at: DateTime<Utc>) -> AppResult<u64> {
        let before = before.to_rfc3339();
        let at = at.to_rfc3339();
        self.db.run(move|c|Ok(c.execute("UPDATE remote_automation_run_requests SET status='failedStale',updated_at=?1 WHERE status='starting' AND claimed_at < ?2",params![at,before])? as u64)).await
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
    db.run(move|c|{c.execute("UPDATE remote_automation_run_requests SET status=?1,result_json=?2,error_code=?3,updated_at=?4 WHERE id=?5 AND status='starting'",params![status,result,error,at,id])?;Ok(())}).await
}
