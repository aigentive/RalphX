use std::{str::FromStr, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::entities::{
    ProjectId, RemoteExecutionResumeRequest, RemoteRecoveryAction, RemoteResumeRequestStatus,
    RemoteTaskAction, RemoteTaskActionRequest, TaskId,
};
use crate::domain::repositories::{
    RemoteExecutionResumeRequestRepository, RemoteTaskActionRequestRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

#[cfg(test)]
#[path = "sqlite_remote_resume_request_repo_tests.rs"]
mod tests;

const COLUMNS: &str = "id, action, task_id, project_id, group_kind, group_id, force_restart, note, recovery_action, status, error_code, result_json, claimed_at, created_at, updated_at";

fn parse_time(value: &str, index: usize) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}

fn parse_status(value: String) -> rusqlite::Result<RemoteResumeRequestStatus> {
    RemoteResumeRequestStatus::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            9,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        )
    })
}

fn parse_result(value: Option<String>) -> rusqlite::Result<Option<serde_json::Value>> {
    value
        .map(|json| {
            serde_json::from_str(&json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    11,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn execution_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RemoteExecutionResumeRequest> {
    let claimed: Option<String> = row.get("claimed_at")?;
    Ok(RemoteExecutionResumeRequest {
        id: row.get("id")?,
        project_id: row
            .get::<_, Option<String>>("project_id")?
            .map(ProjectId::from_string),
        status: parse_status(row.get("status")?)?,
        error_code: row.get("error_code")?,
        result: parse_result(row.get("result_json")?)?,
        claimed_at: claimed.map(|value| parse_time(&value, 12)).transpose()?,
        created_at: parse_time(&row.get::<_, String>("created_at")?, 13)?,
        updated_at: parse_time(&row.get::<_, String>("updated_at")?, 14)?,
    })
}

fn task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RemoteTaskActionRequest> {
    let action = RemoteTaskAction::from_str(&row.get::<_, String>("action")?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        )
    })?;
    let claimed: Option<String> = row.get("claimed_at")?;
    Ok(RemoteTaskActionRequest {
        id: row.get("id")?,
        action,
        task_id: row
            .get::<_, Option<String>>("task_id")?
            .map(TaskId::from_string),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        group_kind: row.get("group_kind")?,
        group_id: row.get("group_id")?,
        force: row.get::<_, i64>("force_restart")? != 0,
        note: row.get("note")?,
        recovery_action: row
            .get::<_, Option<String>>("recovery_action")?
            .map(|value| RemoteRecoveryAction::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
                )
            })?,
        status: parse_status(row.get("status")?)?,
        error_code: row.get("error_code")?,
        result: parse_result(row.get("result_json")?)?,
        claimed_at: claimed.map(|value| parse_time(&value, 12)).transpose()?,
        created_at: parse_time(&row.get::<_, String>("created_at")?, 13)?,
        updated_at: parse_time(&row.get::<_, String>("updated_at")?, 14)?,
    })
}

pub struct SqliteRemoteExecutionResumeRequestRepository {
    db: DbConnection,
}
pub struct SqliteRemoteTaskActionRequestRepository {
    db: DbConnection,
}
impl SqliteRemoteExecutionResumeRequestRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}
impl SqliteRemoteTaskActionRequestRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

async fn settle(
    db: &DbConnection,
    family: &'static str,
    id: &str,
    status: &'static str,
    result: Option<serde_json::Value>,
    error: Option<String>,
    at: DateTime<Utc>,
) -> AppResult<()> {
    let id = id.to_string();
    let result = result.map(|value| value.to_string());
    let at = at.to_rfc3339();
    db.run(move|conn|{conn.execute("UPDATE remote_resume_requests SET status=?1,result_json=?2,error_code=?3,updated_at=?4 WHERE id=?5 AND family=?6 AND status='starting'",params![status,result,error,at,id,family])?;Ok(())}).await
}
async fn sweep(
    db: &DbConnection,
    family: &'static str,
    before: DateTime<Utc>,
    at: DateTime<Utc>,
) -> AppResult<u64> {
    let before = before.to_rfc3339();
    let at = at.to_rfc3339();
    db.run(move|conn|Ok(conn.execute("UPDATE remote_resume_requests SET status='failedStale',updated_at=?1 WHERE family=?2 AND status='starting' AND claimed_at < ?3",params![at,family,before])? as u64)).await
}

#[async_trait]
impl RemoteExecutionResumeRequestRepository for SqliteRemoteExecutionResumeRequestRepository {
    async fn create_execution_resume_request(
        &self,
        r: RemoteExecutionResumeRequest,
    ) -> AppResult<RemoteExecutionResumeRequest> {
        let stored = r.clone();
        self.db.run(move|c|{c.execute("INSERT INTO remote_resume_requests(id,family,project_id,status,error_code,result_json,claimed_at,created_at,updated_at)VALUES(?1,'execution',?2,?3,?4,?5,?6,?7,?8)",params![r.id,r.project_id.as_ref().map(ProjectId::as_str),r.status.as_db_str(),r.error_code,r.result.map(|v|v.to_string()),r.claimed_at.map(|v|v.to_rfc3339()),r.created_at.to_rfc3339(),r.updated_at.to_rfc3339()])?;Ok(())}).await?;
        Ok(stored)
    }
    async fn get(&self, id: &str) -> AppResult<Option<RemoteExecutionResumeRequest>> {
        let id = id.to_string();
        self.db.run(move|c|c.query_row(&format!("SELECT {COLUMNS} FROM remote_resume_requests WHERE family='execution' AND id=?1"),params![id],execution_row).optional().map_err(AppError::from)).await
    }
    async fn find_unsettled(
        &self,
        p: Option<&ProjectId>,
    ) -> AppResult<Option<RemoteExecutionResumeRequest>> {
        let p = p.map(|id| id.as_str().to_string());
        self.db.run(move|c|c.query_row(&format!("SELECT {COLUMNS} FROM remote_resume_requests WHERE family='execution' AND project_id IS ?1 AND status IN('pending','starting') ORDER BY created_at LIMIT 1"),params![p],execution_row).optional().map_err(AppError::from)).await
    }
    async fn claim_pending(
        &self,
        at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteExecutionResumeRequest>> {
        let at = at.to_rfc3339();
        self.db.run_transaction(move|c|{let id:Option<String>=c.query_row("SELECT id FROM remote_resume_requests WHERE family='execution' AND status='pending' ORDER BY created_at,id LIMIT 1",[],|r|r.get(0)).optional()?;let Some(id)=id else{return Ok(None)};if c.execute("UPDATE remote_resume_requests SET status='starting',claimed_at=?1,updated_at=?1 WHERE id=?2 AND status='pending'",params![at,id])?==0{return Ok(None)};Ok(c.query_row(&format!("SELECT {COLUMNS} FROM remote_resume_requests WHERE id=?1"),params![id],execution_row).optional()?)}).await
    }
    async fn complete(&self, id: &str, r: serde_json::Value, at: DateTime<Utc>) -> AppResult<()> {
        settle(&self.db, "execution", id, "completed", Some(r), None, at).await
    }
    async fn fail(&self, id: &str, e: &str, at: DateTime<Utc>) -> AppResult<()> {
        settle(
            &self.db,
            "execution",
            id,
            "failed",
            None,
            Some(e.to_string()),
            at,
        )
        .await
    }
    async fn fail_stale(&self, b: DateTime<Utc>, a: DateTime<Utc>) -> AppResult<u64> {
        sweep(&self.db, "execution", b, a).await
    }
}

#[async_trait]
impl RemoteTaskActionRequestRepository for SqliteRemoteTaskActionRequestRepository {
    async fn create_task_action_request(
        &self,
        r: RemoteTaskActionRequest,
    ) -> AppResult<RemoteTaskActionRequest> {
        let stored = r.clone();
        self.db.run(move|c|{c.execute("INSERT INTO remote_resume_requests(id,family,action,task_id,project_id,group_kind,group_id,force_restart,note,recovery_action,status,error_code,result_json,claimed_at,created_at,updated_at)VALUES(?1,'task',?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",params![r.id,r.action.as_db_str(),r.task_id.as_ref().map(TaskId::as_str),r.project_id.as_str(),r.group_kind,r.group_id,r.force as i64,r.note,r.recovery_action.map(RemoteRecoveryAction::as_db_str),r.status.as_db_str(),r.error_code,r.result.map(|v|v.to_string()),r.claimed_at.map(|v|v.to_rfc3339()),r.created_at.to_rfc3339(),r.updated_at.to_rfc3339()])?;Ok(())}).await?;
        Ok(stored)
    }
    async fn get(&self, id: &str) -> AppResult<Option<RemoteTaskActionRequest>> {
        let id = id.to_string();
        self.db.run(move|c|c.query_row(&format!("SELECT {COLUMNS} FROM remote_resume_requests WHERE family='task' AND id=?1"),params![id],task_row).optional().map_err(AppError::from)).await
    }
    async fn find_unsettled_for_task(
        &self,
        t: &TaskId,
    ) -> AppResult<Option<RemoteTaskActionRequest>> {
        let t = t.as_str().to_string();
        self.db.run(move|c|c.query_row(&format!("SELECT {COLUMNS} FROM remote_resume_requests WHERE family='task' AND task_id=?1 AND status IN('pending','starting') ORDER BY created_at LIMIT 1"),params![t],task_row).optional().map_err(AppError::from)).await
    }
    async fn find_unsettled_for_group(
        &self,
        p: &ProjectId,
        k: &str,
        g: &str,
    ) -> AppResult<Option<RemoteTaskActionRequest>> {
        let p = p.as_str().to_string();
        let k = k.to_string();
        let g = g.to_string();
        self.db.run(move|c|c.query_row(&format!("SELECT {COLUMNS} FROM remote_resume_requests WHERE family='task' AND project_id=?1 AND group_kind=?2 AND group_id=?3 AND status IN('pending','starting') ORDER BY created_at LIMIT 1"),params![p,k,g],task_row).optional().map_err(AppError::from)).await
    }
    async fn claim_pending(&self, at: DateTime<Utc>) -> AppResult<Option<RemoteTaskActionRequest>> {
        let at = at.to_rfc3339();
        self.db.run_transaction(move|c|{let id:Option<String>=c.query_row("SELECT id FROM remote_resume_requests WHERE family='task' AND status='pending' ORDER BY created_at,id LIMIT 1",[],|r|r.get(0)).optional()?;let Some(id)=id else{return Ok(None)};if c.execute("UPDATE remote_resume_requests SET status='starting',claimed_at=?1,updated_at=?1 WHERE id=?2 AND status='pending'",params![at,id])?==0{return Ok(None)};Ok(c.query_row(&format!("SELECT {COLUMNS} FROM remote_resume_requests WHERE id=?1"),params![id],task_row).optional()?)}).await
    }
    async fn complete(&self, id: &str, r: serde_json::Value, at: DateTime<Utc>) -> AppResult<()> {
        settle(&self.db, "task", id, "completed", Some(r), None, at).await
    }
    async fn fail(&self, id: &str, e: &str, at: DateTime<Utc>) -> AppResult<()> {
        settle(
            &self.db,
            "task",
            id,
            "failed",
            None,
            Some(e.to_string()),
            at,
        )
        .await
    }
    async fn fail_stale(&self, b: DateTime<Utc>, a: DateTime<Utc>) -> AppResult<u64> {
        sweep(&self.db, "task", b, a).await
    }
}
