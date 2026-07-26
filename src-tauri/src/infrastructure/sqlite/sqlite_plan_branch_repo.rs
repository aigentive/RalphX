// SQLite-based PlanBranchRepository implementation

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};

use super::DbConnection;
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus};
use crate::domain::entities::{
    ArtifactId, ExecutionPlanId, IdeationSessionId, PlanBranch, PlanBranchId, PlanBranchStatus,
    ProjectId, TaskId,
};
use crate::domain::repositories::PlanBranchRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::git_runtime_config;

pub struct SqlitePlanBranchRepository {
    db: DbConnection,
}

impl SqlitePlanBranchRepository {
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
impl PlanBranchRepository for SqlitePlanBranchRepository {
    async fn create(&self, branch: PlanBranch) -> AppResult<PlanBranch> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch, status, merge_task_id, created_at, merged_at, execution_plan_id, pr_number, pr_eligible, base_branch_override)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    rusqlite::params![
                        branch.id.as_str(),
                        branch.plan_artifact_id.as_str(),
                        branch.session_id.as_str(),
                        branch.project_id.as_str(),
                        branch.branch_name,
                        branch.source_branch,
                        branch.status.to_db_string(),
                        branch.merge_task_id.as_ref().map(|t| t.as_str().to_string()),
                        branch.created_at.to_rfc3339(),
                        branch.merged_at.map(|dt| dt.to_rfc3339()),
                        branch.execution_plan_id.as_ref().map(|id| id.as_str().to_string()),
                        branch.pr_number,
                        branch.pr_eligible as i64,
                        branch.base_branch_override,
                    ],
                )
                .map_err(|e| AppError::Database(format!("Failed to create plan branch: {}", e)))?;
                Ok(branch)
            })
            .await
    }

    async fn create_or_update(&self, branch: PlanBranch) -> AppResult<PlanBranch> {
        self.db
            .run(move |conn| {
                let session_id = branch.session_id.as_str().to_string();
                conn.execute(
                    "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch, status, merge_task_id, created_at, merged_at, execution_plan_id, pr_number, pr_eligible, base_branch_override)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                     ON CONFLICT(session_id) DO UPDATE SET
                       plan_artifact_id=excluded.plan_artifact_id,
                       project_id=excluded.project_id,
                       branch_name=excluded.branch_name,
                       source_branch=excluded.source_branch,
                       status=excluded.status,
                       merge_task_id=excluded.merge_task_id,
                       execution_plan_id=excluded.execution_plan_id,
                       pr_eligible=excluded.pr_eligible,
                       base_branch_override=excluded.base_branch_override",
                    rusqlite::params![
                        branch.id.as_str(),
                        branch.plan_artifact_id.as_str(),
                        branch.session_id.as_str(),
                        branch.project_id.as_str(),
                        branch.branch_name,
                        branch.source_branch,
                        branch.status.to_db_string(),
                        branch.merge_task_id.as_ref().map(|t| t.as_str().to_string()),
                        branch.created_at.to_rfc3339(),
                        branch.merged_at.map(|dt| dt.to_rfc3339()),
                        branch.execution_plan_id.as_ref().map(|id| id.as_str().to_string()),
                        branch.pr_number,
                        branch.pr_eligible as i64,
                        branch.base_branch_override,
                    ],
                )
                .map_err(|e| AppError::Database(format!("Failed to upsert plan branch: {}", e)))?;
                // Re-fetch by session_id so callers always receive the persisted row's id,
                // even when ON CONFLICT fired and the existing row's id was preserved.
                let mut stmt = conn
                    .prepare("SELECT * FROM plan_branches WHERE session_id = ?1")
                    .map_err(|e| AppError::Database(format!("Failed to prepare query: {}", e)))?;
                stmt.query_row(rusqlite::params![session_id.as_str()], |row| {
                    PlanBranch::from_row(row)
                })
                .map_err(|e| AppError::Database(format!("Failed to fetch upserted plan branch: {}", e)))
            })
            .await
    }

    async fn get_by_id(&self, id: &PlanBranchId) -> AppResult<Option<PlanBranch>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare("SELECT * FROM plan_branches WHERE id = ?1")
                    .map_err(|e| AppError::Database(format!("Failed to prepare query: {}", e)))?;
                let result = stmt.query_row(rusqlite::params![id.as_str()], |row| {
                    PlanBranch::from_row(row)
                });
                match result {
                    Ok(branch) => Ok(Some(branch)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(format!(
                        "Failed to get plan branch by id: {}",
                        e
                    ))),
                }
            })
            .await
    }

    async fn get_by_execution_plan_id(
        &self,
        id: &ExecutionPlanId,
    ) -> AppResult<Option<PlanBranch>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare("SELECT * FROM plan_branches WHERE execution_plan_id = ?1")
                    .map_err(|e| AppError::Database(format!("Failed to prepare query: {}", e)))?;
                let result = stmt.query_row(rusqlite::params![id.as_str()], |row| {
                    PlanBranch::from_row(row)
                });
                match result {
                    Ok(branch) => Ok(Some(branch)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(format!(
                        "Failed to get plan branch by execution plan id: {}",
                        e
                    ))),
                }
            })
            .await
    }

    async fn get_by_plan_artifact_id(&self, id: &ArtifactId) -> AppResult<Vec<PlanBranch>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare("SELECT * FROM plan_branches WHERE plan_artifact_id = ?1")
                    .map_err(|e| AppError::Database(format!("Failed to prepare query: {}", e)))?;
                let branches = stmt
                    .query_map(rusqlite::params![id.as_str()], |row| {
                        PlanBranch::from_row(row)
                    })
                    .map_err(|e| {
                        AppError::Database(format!(
                            "Failed to query plan branches by artifact id: {}",
                            e
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        AppError::Database(format!(
                            "Failed to collect plan branches by artifact id: {}",
                            e
                        ))
                    })?;
                Ok(branches)
            })
            .await
    }

    async fn get_by_session_id(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<PlanBranch>> {
        let session_id = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare("SELECT * FROM plan_branches WHERE session_id = ?1")
                    .map_err(|e| AppError::Database(format!("Failed to prepare query: {}", e)))?;
                let result = stmt.query_row(rusqlite::params![session_id.as_str()], |row| {
                    PlanBranch::from_row(row)
                });
                match result {
                    Ok(branch) => Ok(Some(branch)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(format!(
                        "Failed to get plan branch by session id: {}",
                        e
                    ))),
                }
            })
            .await
    }

    async fn get_by_merge_task_id(&self, task_id: &TaskId) -> AppResult<Option<PlanBranch>> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare("SELECT * FROM plan_branches WHERE merge_task_id = ?1")
                    .map_err(|e| AppError::Database(format!("Failed to prepare query: {}", e)))?;
                let result = stmt.query_row(rusqlite::params![task_id.as_str()], |row| {
                    PlanBranch::from_row(row)
                });
                match result {
                    Ok(branch) => Ok(Some(branch)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(format!(
                        "Failed to get plan branch by merge task id: {}",
                        e
                    ))),
                }
            })
            .await
    }

    async fn get_by_project_id(&self, project_id: &ProjectId) -> AppResult<Vec<PlanBranch>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT * FROM plan_branches WHERE project_id = ?1 ORDER BY created_at DESC",
                    )
                    .map_err(|e| {
                        AppError::Database(format!("Failed to prepare query: {}", e))
                    })?;
                let branches = stmt
                    .query_map(rusqlite::params![project_id.as_str()], |row| {
                        PlanBranch::from_row(row)
                    })
                    .map_err(|e| {
                        AppError::Database(format!("Failed to query plan branches: {}", e))
                    })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        AppError::Database(format!("Failed to collect plan branches: {}", e))
                    })?;
                Ok(branches)
            })
            .await
    }

    async fn get_startup_pr_recovery_candidates_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<PlanBranch>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT * FROM plan_branches
                         WHERE project_id = ?1
                           AND status = 'active'
                           AND merge_task_id IS NOT NULL
                           AND (pr_eligible = 1 OR pr_number IS NOT NULL)
                         ORDER BY created_at DESC",
                    )
                    .map_err(|e| {
                        AppError::Database(format!(
                            "Failed to prepare startup PR recovery query: {}",
                            e
                        ))
                    })?;
                let branches = stmt
                    .query_map(rusqlite::params![project_id.as_str()], |row| {
                        PlanBranch::from_row(row)
                    })
                    .map_err(|e| {
                        AppError::Database(format!(
                            "Failed to query startup PR recovery branches: {}",
                            e
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        AppError::Database(format!(
                            "Failed to collect startup PR recovery branches: {}",
                            e
                        ))
                    })?;
                Ok(branches)
            })
            .await
    }

    async fn get_terminal_local_cleanup_candidates_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<PlanBranch>> {
        let project_id = project_id.as_str().to_string();
        let retry_secs = git_runtime_config()
            .terminal_pr_local_cleanup_retry_secs
            .min(i64::MAX as u64) as i64;
        let retry_cutoff = (Utc::now() - chrono::Duration::seconds(retry_secs)).to_rfc3339();
        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT * FROM plan_branches
                         WHERE project_id = ?1
                           AND (status = 'merged' OR pr_status = 'Merged')
                           AND (
                             local_cleanup_status IS NULL
                             OR (
                               local_cleanup_status IN ('unsafe', 'target_ref_missing', 'workspace_dirty')
                               AND local_cleanup_checked_at IS NOT NULL
                               AND local_cleanup_checked_at < ?2
                             )
                           )
                         ORDER BY created_at DESC",
                    )
                    .map_err(|e| {
                        AppError::Database(format!(
                            "Failed to prepare terminal cleanup query: {}",
                            e
                        ))
                    })?;
                let branches = stmt
                    .query_map(rusqlite::params![project_id.as_str(), retry_cutoff], |row| {
                        PlanBranch::from_row(row)
                    })
                    .map_err(|e| {
                        AppError::Database(format!(
                            "Failed to query terminal cleanup plan branches: {}",
                            e
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        AppError::Database(format!(
                            "Failed to collect terminal cleanup plan branches: {}",
                            e
                        ))
                    })?;
                Ok(branches)
            })
            .await
    }

    async fn mark_local_cleanup_status(
        &self,
        id: &PlanBranchId,
        status: &str,
        checked_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        let status = status.to_string();
        let checked_at = checked_at.to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE plan_branches
                     SET local_cleanup_status = ?1, local_cleanup_checked_at = ?2
                     WHERE id = ?3",
                    rusqlite::params![status, checked_at, id.as_str()],
                )
                .map_err(|e| {
                    AppError::Database(format!(
                        "Failed to mark plan branch local cleanup status: {}",
                        e
                    ))
                })?;
                Ok(())
            })
            .await
    }

    async fn get_local_cleanup_status(&self, id: &PlanBranchId) -> AppResult<Option<String>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT local_cleanup_status FROM plan_branches WHERE id = ?1",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .optional()
                .map(|value| value.flatten())
                .map_err(AppError::from)
            })
            .await
    }

    async fn clear_local_cleanup_status(&self, id: &PlanBranchId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE plan_branches
                     SET local_cleanup_status = NULL, local_cleanup_checked_at = NULL
                     WHERE id = ?1",
                    rusqlite::params![id],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_status(&self, id: &PlanBranchId, status: PlanBranchStatus) -> AppResult<()> {
        let id = id.as_str().to_string();
        let id_display = id.clone();
        self.db
            .run(move |conn| {
                let rows = conn
                    .execute(
                        "UPDATE plan_branches SET status = ?1 WHERE id = ?2",
                        rusqlite::params![status.to_db_string(), id.as_str()],
                    )
                    .map_err(|e| {
                        AppError::Database(format!("Failed to update plan branch status: {}", e))
                    })?;
                if rows == 0 {
                    return Err(AppError::NotFound(format!(
                        "Plan branch not found: {}",
                        id_display
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn update_pr_eligible(&self, id: &PlanBranchId, enabled: bool) -> AppResult<()> {
        let id = id.as_str().to_string();
        let id_display = id.clone();
        self.db
            .run(move |conn| {
                let rows = conn
                    .execute(
                        "UPDATE plan_branches SET pr_eligible = ?1 WHERE id = ?2",
                        rusqlite::params![enabled as i64, id.as_str()],
                    )
                    .map_err(|e| {
                        AppError::Database(format!(
                            "Failed to update plan branch pr_eligible: {}",
                            e
                        ))
                    })?;
                if rows == 0 {
                    return Err(AppError::NotFound(format!(
                        "Plan branch not found: {}",
                        id_display
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn set_merge_task_id(&self, id: &PlanBranchId, task_id: &TaskId) -> AppResult<()> {
        let id = id.as_str().to_string();
        let task_id = task_id.as_str().to_string();
        let id_display = id.clone();
        self.db
            .run(move |conn| {
                let rows = conn
                    .execute(
                        "UPDATE plan_branches SET merge_task_id = ?1 WHERE id = ?2",
                        rusqlite::params![task_id.as_str(), id.as_str()],
                    )
                    .map_err(|e| {
                        AppError::Database(format!("Failed to set merge task id: {}", e))
                    })?;
                if rows == 0 {
                    return Err(AppError::NotFound(format!(
                        "Plan branch not found: {}",
                        id_display
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn clear_merge_task_id(&self, id: &PlanBranchId) -> AppResult<()> {
        let id = id.as_str().to_string();
        let id_display = id.clone();
        self.db
            .run(move |conn| {
                let rows = conn
                    .execute(
                        "UPDATE plan_branches SET merge_task_id = NULL WHERE id = ?1",
                        rusqlite::params![id.as_str()],
                    )
                    .map_err(|e| {
                        AppError::Database(format!("Failed to clear merge task id: {}", e))
                    })?;
                if rows == 0 {
                    return Err(AppError::NotFound(format!(
                        "Plan branch not found: {}",
                        id_display
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn set_merged(&self, id: &PlanBranchId) -> AppResult<()> {
        let id = id.as_str().to_string();
        let id_display = id.clone();
        self.db
            .run(move |conn| {
                let rows = conn
                    .execute(
                        "UPDATE plan_branches SET status = 'merged', merged_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now') WHERE id = ?1",
                        rusqlite::params![id.as_str()],
                    )
                    .map_err(|e| {
                        AppError::Database(format!("Failed to set plan branch merged: {}", e))
                    })?;
                if rows == 0 {
                    return Err(AppError::NotFound(format!(
                        "Plan branch not found: {}",
                        id_display
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn abandon_active_for_artifact(&self, artifact_id: &ArtifactId) -> AppResult<u32> {
        let artifact_id = artifact_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let rows = conn
                    .execute(
                        "UPDATE plan_branches SET status = 'abandoned' WHERE plan_artifact_id = ?1 AND status = 'active'",
                        rusqlite::params![artifact_id.as_str()],
                    )
                    .map_err(|e| {
                        AppError::Database(format!("Failed to abandon active plan branches: {}", e))
                    })?;
                Ok(rows as u32)
            })
            .await
    }

    async fn delete(&self, id: &PlanBranchId) -> AppResult<()> {
        let id = id.as_str().to_string();
        let id_display = id.clone();
        self.db
            .run(move |conn| {
                let rows = conn
                    .execute(
                        "DELETE FROM plan_branches WHERE id = ?1",
                        rusqlite::params![id.as_str()],
                    )
                    .map_err(|e| {
                        AppError::Database(format!("Failed to delete plan branch: {}", e))
                    })?;
                if rows == 0 {
                    return Err(AppError::NotFound(format!(
                        "Plan branch not found: {}",
                        id_display
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn update_pr_info(
        &self,
        id: &PlanBranchId,
        pr_number: i64,
        pr_url: String,
        pr_status: PrStatus,
        pr_draft: bool,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE plan_branches SET pr_number = ?1, pr_url = ?2, pr_status = ?3, pr_draft = ?4, pr_push_status = 'pushed' WHERE id = ?5",
                    rusqlite::params![
                        pr_number,
                        pr_url,
                        pr_status.to_db_string(),
                        pr_draft as i64,
                        id.as_str(),
                    ],
                )
                .map_err(|e| AppError::Database(format!("Failed to update PR info: {}", e)))?;
                Ok(())
            })
            .await
    }

    async fn clear_pr_info(&self, id: &PlanBranchId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE plan_branches SET pr_number = NULL, pr_url = NULL, pr_status = NULL, pr_draft = NULL, pr_push_status = 'pending', pr_polling_active = 0, last_polled_at = NULL, merge_commit_sha = NULL WHERE id = ?1",
                    rusqlite::params![id.as_str()],
                )
                .map_err(|e| AppError::Database(format!("Failed to clear PR info: {}", e)))?;
                Ok(())
            })
            .await
    }

    async fn update_pr_status(&self, id: &PlanBranchId, status: PrStatus) -> AppResult<()> {
        let id = id.as_str().to_string();
        let status_str = status.to_db_string().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE plan_branches SET pr_status = ?1 WHERE id = ?2",
                    rusqlite::params![status_str, id.as_str()],
                )
                .map_err(|e| AppError::Database(format!("Failed to update PR status: {}", e)))?;
                Ok(())
            })
            .await
    }

    async fn set_merge_commit_sha(&self, id: &PlanBranchId, sha: String) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE plan_branches SET merge_commit_sha = ?1 WHERE id = ?2",
                    rusqlite::params![sha, id.as_str()],
                )
                .map_err(|e| {
                    AppError::Database(format!("Failed to set merge commit sha: {}", e))
                })?;
                Ok(())
            })
            .await
    }

    async fn update_last_polled_at(
        &self,
        id: &PlanBranchId,
        polled_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        let ts = polled_at.to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE plan_branches SET last_polled_at = ?1, pr_polling_active = 1 WHERE id = ?2",
                    rusqlite::params![ts, id.as_str()],
                )
                .map_err(|e| {
                    AppError::Database(format!("Failed to update last polled at: {}", e))
                })?;
                Ok(())
            })
            .await
    }

    async fn clear_polling_active_by_task(&self, task_id: &TaskId) -> AppResult<()> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE plan_branches SET pr_polling_active = 0 WHERE merge_task_id = ?1",
                    rusqlite::params![task_id.as_str()],
                )
                .map_err(|e| {
                    AppError::Database(format!("Failed to clear polling active: {}", e))
                })?;
                Ok(())
            })
            .await
    }

    async fn find_pr_polling_task_ids(&self) -> AppResult<Vec<TaskId>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT merge_task_id FROM plan_branches WHERE pr_polling_active = 1 AND merge_task_id IS NOT NULL",
                    )
                    .map_err(|e| AppError::Database(format!("Failed to prepare query: {}", e)))?;
                let ids = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| {
                        AppError::Database(format!("Failed to find polling task ids: {}", e))
                    })?
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(TaskId::from_string)
                    .collect();
                Ok(ids)
            })
            .await
    }

    async fn update_pr_push_status(
        &self,
        id: &PlanBranchId,
        status: PrPushStatus,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let status_str = status.to_db_string().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE plan_branches SET pr_push_status = ?1 WHERE id = ?2",
                    rusqlite::params![status_str, id_str],
                )
                .map_err(|e| {
                    AppError::Database(format!("Failed to update pr_push_status: {}", e))
                })?;
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_plan_branch_repo_tests.rs"]
mod tests;
