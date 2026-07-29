use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::entities::{ProjectId, TicketCanonicalBranch};
use crate::domain::repositories::TicketCanonicalBranchRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

#[cfg(test)]
#[path = "sqlite_ticket_canonical_branch_repo_tests.rs"]
mod tests;

fn parse_datetime(value: &str) -> DateTime<Utc> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&dt);
    }
    Utc::now()
}

fn row_to_branch(row: &rusqlite::Row<'_>) -> rusqlite::Result<TicketCanonicalBranch> {
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;

    Ok(TicketCanonicalBranch {
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        provider: row.get("provider")?,
        issue_key: row.get("issue_key")?,
        branch_name: row.get("branch_name")?,
        base_branch: row.get("base_branch")?,
        base_commit: row.get("base_commit")?,
        origin_pushed: row.get("origin_pushed")?,
        terminal: row.get("terminal")?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
    })
}

pub struct SqliteTicketCanonicalBranchRepository {
    db: DbConnection,
}

impl SqliteTicketCanonicalBranchRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

#[async_trait]
impl TicketCanonicalBranchRepository for SqliteTicketCanonicalBranchRepository {
    async fn get(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
    ) -> AppResult<Option<TicketCanonicalBranch>> {
        let project_id = project_id.as_str().to_string();
        let provider = provider.to_string();
        let issue_key = issue_key.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM ticket_canonical_branches
                     WHERE project_id = ?1 AND provider = ?2 AND issue_key = ?3",
                    params![project_id, provider, issue_key],
                    row_to_branch,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn upsert(
        &self,
        branch: TicketCanonicalBranch,
    ) -> AppResult<TicketCanonicalBranch> {
        let fetch_project_id = branch.project_id.as_str().to_string();
        let fetch_provider = branch.provider.clone();
        let fetch_issue_key = branch.issue_key.clone();
        self.db
            .run(move |conn| {
                let updated_at = Utc::now().to_rfc3339();
                conn.execute(
                    "INSERT INTO ticket_canonical_branches (
                        project_id, provider, issue_key, branch_name, base_branch, base_commit,
                        origin_pushed, terminal, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                    ON CONFLICT(project_id, provider, issue_key) DO UPDATE SET
                        branch_name=excluded.branch_name,
                        base_branch=excluded.base_branch,
                        base_commit=excluded.base_commit,
                        origin_pushed=excluded.origin_pushed,
                        terminal=excluded.terminal,
                        updated_at=excluded.updated_at",
                    params![
                        branch.project_id.as_str(),
                        &branch.provider,
                        &branch.issue_key,
                        &branch.branch_name,
                        &branch.base_branch,
                        branch.base_commit.as_deref(),
                        branch.origin_pushed,
                        branch.terminal,
                        branch.created_at.to_rfc3339(),
                        updated_at,
                    ],
                )?;
                conn.query_row(
                    "SELECT * FROM ticket_canonical_branches
                     WHERE project_id = ?1 AND provider = ?2 AND issue_key = ?3",
                    params![fetch_project_id, fetch_provider, fetch_issue_key],
                    row_to_branch,
                )
                .map_err(AppError::from)
            })
            .await
    }

    async fn mark_origin_pushed(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
    ) -> AppResult<()> {
        let project_id = project_id.as_str().to_string();
        let provider = provider.to_string();
        let issue_key = issue_key.to_string();
        self.db
            .run(move |conn| {
                let updated_at = Utc::now().to_rfc3339();
                conn.execute(
                    "UPDATE ticket_canonical_branches
                     SET origin_pushed = 1, updated_at = ?4
                     WHERE project_id = ?1 AND provider = ?2 AND issue_key = ?3",
                    params![project_id, provider, issue_key, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn mark_terminal(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
    ) -> AppResult<()> {
        let project_id = project_id.as_str().to_string();
        let provider = provider.to_string();
        let issue_key = issue_key.to_string();
        self.db
            .run(move |conn| {
                let updated_at = Utc::now().to_rfc3339();
                conn.execute(
                    "UPDATE ticket_canonical_branches
                     SET terminal = 1, updated_at = ?4
                     WHERE project_id = ?1 AND provider = ?2 AND issue_key = ?3",
                    params![project_id, provider, issue_key, updated_at],
                )?;
                Ok(())
            })
            .await
    }
}
