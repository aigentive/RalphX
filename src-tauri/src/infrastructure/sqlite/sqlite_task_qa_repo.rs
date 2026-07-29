// SQLite-based TaskQARepository implementation for production use
// Uses DbConnection for non-blocking SQLite access via spawn_blocking

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::Connection;

use crate::domain::entities::{TaskId, TaskQA, TaskQAId};
use crate::domain::qa::{AcceptanceCriteria, QAResults, QATestSteps};
use crate::domain::repositories::TaskQARepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

/// SQLite implementation of TaskQARepository for production use
pub struct SqliteTaskQARepository {
    pub(crate) db: DbConnection,
}

impl SqliteTaskQARepository {
    /// Create a new SQLite TaskQA repository with the given connection
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    /// Create from an Arc-wrapped mutex connection (for sharing)
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }

    /// Helper to parse datetime from SQLite
    fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
        // Try parsing with common SQLite datetime formats
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
            .ok()
            .map(|ndt| Utc.from_utc_datetime(&ndt))
            .or_else(|| {
                chrono::DateTime::parse_from_rfc3339(s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            })
    }

    /// Helper to format datetime for SQLite
    fn format_datetime(dt: &DateTime<Utc>) -> String {
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    }

    /// Parse TaskQA from database row
    fn row_to_task_qa(
        id: String,
        task_id: String,
        acceptance_criteria: Option<String>,
        qa_test_steps: Option<String>,
        prep_agent_id: Option<String>,
        prep_started_at: Option<String>,
        prep_completed_at: Option<String>,
        actual_implementation: Option<String>,
        refined_test_steps: Option<String>,
        refinement_agent_id: Option<String>,
        refinement_completed_at: Option<String>,
        test_results: Option<String>,
        screenshots: Option<String>,
        test_agent_id: Option<String>,
        test_completed_at: Option<String>,
        created_at: String,
    ) -> AppResult<TaskQA> {
        let acceptance_criteria = acceptance_criteria
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| {
                AppError::Database(format!("JSON parse error for acceptance_criteria: {}", e))
            })?;

        let qa_test_steps = qa_test_steps
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| {
                AppError::Database(format!("JSON parse error for qa_test_steps: {}", e))
            })?;

        let refined_test_steps = refined_test_steps
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| {
                AppError::Database(format!("JSON parse error for refined_test_steps: {}", e))
            })?;

        let test_results = test_results
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| AppError::Database(format!("JSON parse error for test_results: {}", e)))?;

        let screenshots: Vec<String> = screenshots
            .map(|s| serde_json::from_str(&s).unwrap_or_default())
            .unwrap_or_default();

        let created_at = Self::parse_datetime(&created_at).unwrap_or_else(Utc::now);

        Ok(TaskQA {
            id: TaskQAId::from_string(id),
            task_id: TaskId::from_string(task_id),
            acceptance_criteria,
            qa_test_steps,
            prep_agent_id,
            prep_started_at: prep_started_at.and_then(|s| Self::parse_datetime(&s)),
            prep_completed_at: prep_completed_at.and_then(|s| Self::parse_datetime(&s)),
            actual_implementation,
            refined_test_steps,
            refinement_agent_id,
            refinement_completed_at: refinement_completed_at.and_then(|s| Self::parse_datetime(&s)),
            test_results,
            screenshots,
            test_agent_id,
            test_completed_at: test_completed_at.and_then(|s| Self::parse_datetime(&s)),
            created_at,
        })
    }
}

const SELECT_TASK_QA: &str = "SELECT
    id, task_id,
    acceptance_criteria, qa_test_steps, prep_agent_id, prep_started_at, prep_completed_at,
    actual_implementation, refined_test_steps, refinement_agent_id, refinement_completed_at,
    test_results, screenshots, test_agent_id, test_completed_at,
    created_at
FROM task_qa";

fn extract_row(
    row: &rusqlite::Row,
) -> rusqlite::Result<(
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
)> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, Option<String>>(2)?,
        row.get::<_, Option<String>>(3)?,
        row.get::<_, Option<String>>(4)?,
        row.get::<_, Option<String>>(5)?,
        row.get::<_, Option<String>>(6)?,
        row.get::<_, Option<String>>(7)?,
        row.get::<_, Option<String>>(8)?,
        row.get::<_, Option<String>>(9)?,
        row.get::<_, Option<String>>(10)?,
        row.get::<_, Option<String>>(11)?,
        row.get::<_, Option<String>>(12)?,
        row.get::<_, Option<String>>(13)?,
        row.get::<_, Option<String>>(14)?,
        row.get::<_, String>(15)?,
    ))
}

#[async_trait]
impl TaskQARepository for SqliteTaskQARepository {
    async fn create(&self, task_qa: &TaskQA) -> AppResult<()> {
        let acceptance_criteria_json = task_qa
            .acceptance_criteria
            .as_ref()
            .map(|c| serde_json::to_string(c))
            .transpose()
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?;

        let qa_test_steps_json = task_qa
            .qa_test_steps
            .as_ref()
            .map(|s| serde_json::to_string(s))
            .transpose()
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?;

        let refined_test_steps_json = task_qa
            .refined_test_steps
            .as_ref()
            .map(|s| serde_json::to_string(s))
            .transpose()
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?;

        let test_results_json = task_qa
            .test_results
            .as_ref()
            .map(|r| serde_json::to_string(r))
            .transpose()
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?;

        let screenshots_json = if task_qa.screenshots.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&task_qa.screenshots)
                    .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?,
            )
        };

        let id = task_qa.id.as_str().to_string();
        let task_id = task_qa.task_id.as_str().to_string();
        let prep_agent_id = task_qa.prep_agent_id.clone();
        let prep_started_at = task_qa.prep_started_at.map(|dt| Self::format_datetime(&dt));
        let prep_completed_at = task_qa.prep_completed_at.map(|dt| Self::format_datetime(&dt));
        let actual_implementation = task_qa.actual_implementation.clone();
        let refinement_agent_id = task_qa.refinement_agent_id.clone();
        let refinement_completed_at = task_qa.refinement_completed_at.map(|dt| Self::format_datetime(&dt));
        let test_agent_id = task_qa.test_agent_id.clone();
        let test_completed_at = task_qa.test_completed_at.map(|dt| Self::format_datetime(&dt));
        let created_at = Self::format_datetime(&task_qa.created_at);

        self.db.run(move |conn| {
            conn.execute(
                "INSERT INTO task_qa (
                    id, task_id,
                    acceptance_criteria, qa_test_steps, prep_agent_id, prep_started_at, prep_completed_at,
                    actual_implementation, refined_test_steps, refinement_agent_id, refinement_completed_at,
                    test_results, screenshots, test_agent_id, test_completed_at,
                    created_at
                ) VALUES (
                    ?1, ?2,
                    ?3, ?4, ?5, ?6, ?7,
                    ?8, ?9, ?10, ?11,
                    ?12, ?13, ?14, ?15,
                    ?16
                )",
                rusqlite::params![
                    id, task_id,
                    acceptance_criteria_json, qa_test_steps_json, prep_agent_id,
                    prep_started_at, prep_completed_at,
                    actual_implementation, refined_test_steps_json, refinement_agent_id,
                    refinement_completed_at,
                    test_results_json, screenshots_json, test_agent_id, test_completed_at,
                    created_at,
                ],
            )?;
            Ok(())
        }).await
    }

    async fn get_by_id(&self, id: &TaskQAId) -> AppResult<Option<TaskQA>> {
        let id_str = id.as_str().to_string();
        let result = self.db.query_optional(move |conn| {
            conn.query_row(
                &format!("{} WHERE id = ?1", SELECT_TASK_QA),
                [&id_str],
                extract_row,
            )
        }).await?;

        match result {
            Some((id, task_id, ac, qs, pa, ps, pc, ai, rs, ra, rc, tr, ss, ta, tc, ca)) => {
                Ok(Some(Self::row_to_task_qa(id, task_id, ac, qs, pa, ps, pc, ai, rs, ra, rc, tr, ss, ta, tc, ca)?))
            }
            None => Ok(None),
        }
    }

    async fn get_by_task_id(&self, task_id: &TaskId) -> AppResult<Option<TaskQA>> {
        let task_id_str = task_id.as_str().to_string();
        let result = self.db.query_optional(move |conn| {
            conn.query_row(
                &format!("{} WHERE task_id = ?1", SELECT_TASK_QA),
                [&task_id_str],
                extract_row,
            )
        }).await?;

        match result {
            Some((id, task_id, ac, qs, pa, ps, pc, ai, rs, ra, rc, tr, ss, ta, tc, ca)) => {
                Ok(Some(Self::row_to_task_qa(id, task_id, ac, qs, pa, ps, pc, ai, rs, ra, rc, tr, ss, ta, tc, ca)?))
            }
            None => Ok(None),
        }
    }

    async fn update_prep(
        &self,
        id: &TaskQAId,
        agent_id: &str,
        criteria: &AcceptanceCriteria,
        steps: &QATestSteps,
    ) -> AppResult<()> {
        let criteria_json = serde_json::to_string(criteria)
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?;

        let steps_json = serde_json::to_string(steps)
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?;

        let now = Self::format_datetime(&Utc::now());
        let id_str = id.as_str().to_string();
        let agent_id = agent_id.to_string();

        self.db.run(move |conn| {
            conn.execute(
                "UPDATE task_qa SET
                    prep_agent_id = ?1,
                    acceptance_criteria = ?2,
                    qa_test_steps = ?3,
                    prep_completed_at = ?4
                WHERE id = ?5",
                rusqlite::params![agent_id, criteria_json, steps_json, now, id_str],
            )?;
            Ok(())
        }).await
    }

    async fn update_refinement(
        &self,
        id: &TaskQAId,
        agent_id: &str,
        actual_implementation: &str,
        refined_steps: &QATestSteps,
    ) -> AppResult<()> {
        let steps_json = serde_json::to_string(refined_steps)
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?;

        let now = Self::format_datetime(&Utc::now());
        let id_str = id.as_str().to_string();
        let agent_id = agent_id.to_string();
        let actual_implementation = actual_implementation.to_string();

        self.db.run(move |conn| {
            conn.execute(
                "UPDATE task_qa SET
                    refinement_agent_id = ?1,
                    actual_implementation = ?2,
                    refined_test_steps = ?3,
                    refinement_completed_at = ?4
                WHERE id = ?5",
                rusqlite::params![agent_id, actual_implementation, steps_json, now, id_str],
            )?;
            Ok(())
        }).await
    }

    async fn update_results(
        &self,
        id: &TaskQAId,
        agent_id: &str,
        results: &QAResults,
        screenshots: &[String],
    ) -> AppResult<()> {
        let results_json = serde_json::to_string(results)
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?;

        let screenshots_json = if screenshots.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(screenshots)
                    .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?,
            )
        };

        let now = Self::format_datetime(&Utc::now());
        let id_str = id.as_str().to_string();
        let agent_id = agent_id.to_string();

        self.db.run(move |conn| {
            conn.execute(
                "UPDATE task_qa SET
                    test_agent_id = ?1,
                    test_results = ?2,
                    screenshots = ?3,
                    test_completed_at = ?4
                WHERE id = ?5",
                rusqlite::params![agent_id, results_json, screenshots_json, now, id_str],
            )?;
            Ok(())
        }).await
    }

    async fn get_pending_prep(&self) -> AppResult<Vec<TaskQA>> {
        let rows: Vec<_> = self.db.run(move |conn| {
            let mut stmt = conn.prepare(
                &format!("{} WHERE acceptance_criteria IS NULL", SELECT_TASK_QA),
            )?;
            let rows = stmt
                .query_map([], extract_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        }).await?;

        let mut results = Vec::new();
        for (id, task_id, ac, qs, pa, ps, pc, ai, rs, ra, rc, tr, ss, ta, tc, ca) in rows {
            let task_qa = Self::row_to_task_qa(
                id, task_id, ac, qs, pa, ps, pc, ai, rs, ra, rc, tr, ss, ta, tc, ca,
            )?;
            results.push(task_qa);
        }

        Ok(results)
    }

    async fn delete(&self, id: &TaskQAId) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        self.db.run(move |conn| {
            conn.execute("DELETE FROM task_qa WHERE id = ?1", [id_str])?;
            Ok(())
        }).await
    }

    async fn delete_by_task_id(&self, task_id: &TaskId) -> AppResult<()> {
        let task_id_str = task_id.as_str().to_string();
        self.db.run(move |conn| {
            conn.execute("DELETE FROM task_qa WHERE task_id = ?1", [task_id_str])?;
            Ok(())
        }).await
    }

    async fn exists_for_task(&self, task_id: &TaskId) -> AppResult<bool> {
        let task_id_str = task_id.as_str().to_string();
        self.db.run(move |conn| {
            let count: i32 = conn.query_row(
                "SELECT COUNT(*) FROM task_qa WHERE task_id = ?1",
                [task_id_str],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        }).await
    }
}

#[cfg(test)]
#[path = "sqlite_task_qa_repo_tests.rs"]
mod tests;
