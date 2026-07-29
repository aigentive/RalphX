// SQLite-based ReviewRepository implementation for production use
// Uses rusqlite with connection pooling for thread-safe access

use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::Connection;

use crate::domain::entities::{
    ProjectId, Review, ReviewAction, ReviewActionId, ReviewActionType, ReviewId, ReviewIssue,
    ReviewNote, ReviewNoteId, ReviewOutcome, ReviewStatus, ReviewerType, TaskId,
};
use crate::domain::repositories::ReviewRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

/// SQLite implementation of ReviewRepository for production use
/// Uses a mutex-protected connection for thread-safe access
pub struct SqliteReviewRepository {
    db: DbConnection,
}

impl SqliteReviewRepository {
    /// Create a new SQLite Review repository with the given connection
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

    /// Parse Review from database row
    fn row_to_review(
        id: String,
        project_id: String,
        task_id: String,
        reviewer_type: String,
        status: String,
        notes: Option<String>,
        created_at: String,
        completed_at: Option<String>,
    ) -> AppResult<Review> {
        let reviewer_type = ReviewerType::from_str(&reviewer_type)
            .map_err(|e| AppError::Database(format!("Invalid reviewer_type: {}", e)))?;

        let status = ReviewStatus::from_str(&status)
            .map_err(|e| AppError::Database(format!("Invalid status: {}", e)))?;

        let created_at = Self::parse_datetime(&created_at).unwrap_or_else(Utc::now);

        Ok(Review {
            id: ReviewId::from_string(id),
            project_id: ProjectId::from_string(project_id),
            task_id: TaskId::from_string(task_id),
            reviewer_type,
            status,
            notes,
            created_at,
            completed_at: completed_at.and_then(|s| Self::parse_datetime(&s)),
        })
    }

    /// Parse ReviewAction from database row
    fn row_to_action(
        id: String,
        review_id: String,
        action_type: String,
        target_task_id: Option<String>,
        created_at: String,
    ) -> AppResult<ReviewAction> {
        let action_type = ReviewActionType::from_str(&action_type)
            .map_err(|e| AppError::Database(format!("Invalid action_type: {}", e)))?;

        let created_at = Self::parse_datetime(&created_at).unwrap_or_else(Utc::now);

        Ok(ReviewAction {
            id: ReviewActionId::from_string(id),
            review_id: ReviewId::from_string(review_id),
            action_type,
            target_task_id: target_task_id.map(TaskId::from_string),
            created_at,
        })
    }

    /// Parse ReviewNote from database row
    fn row_to_note(
        id: String,
        task_id: String,
        reviewer: String,
        outcome: String,
        summary: Option<String>,
        notes: Option<String>,
        issues_json: Option<String>,
        followup_session_id: Option<String>,
        created_at: String,
    ) -> AppResult<ReviewNote> {
        let reviewer = ReviewerType::from_str(&reviewer)
            .map_err(|e| AppError::Database(format!("Invalid reviewer: {}", e)))?;

        let outcome = ReviewOutcome::from_str(&outcome)
            .map_err(|e| AppError::Database(format!("Invalid outcome: {}", e)))?;

        let created_at = Self::parse_datetime(&created_at).unwrap_or_else(Utc::now);

        // Parse issues from JSON if present
        let issues =
            issues_json.and_then(|json| serde_json::from_str::<Vec<ReviewIssue>>(&json).ok());

        Ok(ReviewNote {
            id: ReviewNoteId::from_string(id),
            task_id: TaskId::from_string(task_id),
            reviewer,
            outcome,
            summary,
            notes,
            issues,
            followup_session_id,
            created_at,
        })
    }
}

#[async_trait]
impl ReviewRepository for SqliteReviewRepository {
    // ========================================
    // Review methods
    // ========================================

    async fn create(&self, review: &Review) -> AppResult<()> {
        let review = review.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO reviews (id, project_id, task_id, reviewer_type, status, notes, created_at, completed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    (
                        review.id.as_str(),
                        review.project_id.as_str(),
                        review.task_id.as_str(),
                        review.reviewer_type.to_string(),
                        review.status.to_string(),
                        review.notes.as_deref(),
                        SqliteReviewRepository::format_datetime(&review.created_at),
                        review.completed_at.as_ref().map(SqliteReviewRepository::format_datetime),
                    ),
                )
                .map_err(|e| AppError::Database(format!("Failed to create review: {}", e)))?;
                Ok(())
            })
            .await
    }

    async fn get_by_id(&self, id: &ReviewId) -> AppResult<Option<Review>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT id, project_id, task_id, reviewer_type, status, notes, created_at, completed_at
                     FROM reviews WHERE id = ?1",
                    [id.as_str()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, Option<String>>(7)?,
                        ))
                    },
                );

                match result {
                    Ok((id, project_id, task_id, reviewer_type, status, notes, created_at, completed_at)) => {
                        Ok(Some(SqliteReviewRepository::row_to_review(
                            id,
                            project_id,
                            task_id,
                            reviewer_type,
                            status,
                            notes,
                            created_at,
                            completed_at,
                        )?))
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(format!("Failed to get review: {}", e))),
                }
            })
            .await
    }

    async fn get_by_task_id(&self, task_id: &TaskId) -> AppResult<Vec<Review>> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, task_id, reviewer_type, status, notes, created_at, completed_at
                     FROM reviews WHERE task_id = ?1 ORDER BY created_at DESC",
                )
                .map_err(|e| AppError::Database(format!("Failed to prepare statement: {}", e)))?;

                let rows: Vec<(String, String, String, String, String, Option<String>, String, Option<String>)> = stmt
                    .query_map([task_id.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, Option<String>>(7)?,
                        ))
                    })
                    .map_err(|e| AppError::Database(format!("Failed to query reviews: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::Database(format!("Failed to read row: {}", e)))?;

                let mut reviews = Vec::new();
                for (id, project_id, task_id, reviewer_type, status, notes, created_at, completed_at) in rows {
                    reviews.push(SqliteReviewRepository::row_to_review(
                        id,
                        project_id,
                        task_id,
                        reviewer_type,
                        status,
                        notes,
                        created_at,
                        completed_at,
                    )?);
                }
                Ok(reviews)
            })
            .await
    }

    async fn get_pending(&self, project_id: &ProjectId) -> AppResult<Vec<Review>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, task_id, reviewer_type, status, notes, created_at, completed_at
                     FROM reviews WHERE project_id = ?1 AND status = 'pending' ORDER BY created_at ASC",
                )
                .map_err(|e| AppError::Database(format!("Failed to prepare statement: {}", e)))?;

                let rows: Vec<(String, String, String, String, String, Option<String>, String, Option<String>)> = stmt
                    .query_map([project_id.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, Option<String>>(7)?,
                        ))
                    })
                    .map_err(|e| AppError::Database(format!("Failed to query reviews: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::Database(format!("Failed to read row: {}", e)))?;

                let mut reviews = Vec::new();
                for (id, project_id, task_id, reviewer_type, status, notes, created_at, completed_at) in rows {
                    reviews.push(SqliteReviewRepository::row_to_review(
                        id,
                        project_id,
                        task_id,
                        reviewer_type,
                        status,
                        notes,
                        created_at,
                        completed_at,
                    )?);
                }
                Ok(reviews)
            })
            .await
    }

    async fn update(&self, review: &Review) -> AppResult<()> {
        let review = review.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE reviews SET status = ?1, notes = ?2, completed_at = ?3 WHERE id = ?4",
                    (
                        review.status.to_string(),
                        review.notes.as_deref(),
                        review.completed_at.as_ref().map(SqliteReviewRepository::format_datetime),
                        review.id.as_str(),
                    ),
                )
                .map_err(|e| AppError::Database(format!("Failed to update review: {}", e)))?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &ReviewId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM reviews WHERE id = ?1", [id.as_str()])
                    .map_err(|e| AppError::Database(format!("Failed to delete review: {}", e)))?;
                Ok(())
            })
            .await
    }

    // ========================================
    // ReviewAction methods
    // ========================================

    async fn add_action(&self, action: &ReviewAction) -> AppResult<()> {
        let action = action.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO review_actions (id, review_id, action_type, target_task_id, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    (
                        action.id.as_str(),
                        action.review_id.as_str(),
                        action.action_type.to_string(),
                        action.target_task_id.as_ref().map(|id| id.as_str()),
                        SqliteReviewRepository::format_datetime(&action.created_at),
                    ),
                )
                .map_err(|e| AppError::Database(format!("Failed to create review action: {}", e)))?;
                Ok(())
            })
            .await
    }

    async fn get_actions(&self, review_id: &ReviewId) -> AppResult<Vec<ReviewAction>> {
        let review_id = review_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, review_id, action_type, target_task_id, created_at
                     FROM review_actions WHERE review_id = ?1 ORDER BY created_at ASC",
                )
                .map_err(|e| AppError::Database(format!("Failed to prepare statement: {}", e)))?;

                let rows: Vec<(String, String, String, Option<String>, String)> = stmt
                    .query_map([review_id.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    })
                    .map_err(|e| AppError::Database(format!("Failed to query actions: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::Database(format!("Failed to read row: {}", e)))?;

                let mut actions = Vec::new();
                for (id, review_id, action_type, target_task_id, created_at) in rows {
                    actions.push(SqliteReviewRepository::row_to_action(
                        id,
                        review_id,
                        action_type,
                        target_task_id,
                        created_at,
                    )?);
                }
                Ok(actions)
            })
            .await
    }

    async fn get_action_by_id(&self, id: &ReviewActionId) -> AppResult<Option<ReviewAction>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT id, review_id, action_type, target_task_id, created_at
                     FROM review_actions WHERE id = ?1",
                    [id.as_str()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    },
                );

                match result {
                    Ok((id, review_id, action_type, target_task_id, created_at)) => Ok(Some(
                        SqliteReviewRepository::row_to_action(
                            id,
                            review_id,
                            action_type,
                            target_task_id,
                            created_at,
                        )?,
                    )),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(format!("Failed to get action: {}", e))),
                }
            })
            .await
    }

    // ========================================
    // ReviewNote methods
    // ========================================

    async fn add_note(&self, note: &ReviewNote) -> AppResult<()> {
        let note = note.clone();
        self.db
            .run(move |conn| {
                // Serialize issues to JSON if present
                let issues_json = note
                    .issues
                    .as_ref()
                    .and_then(|issues| serde_json::to_string(issues).ok());

                conn.execute(
                    "INSERT INTO review_notes (id, task_id, reviewer, outcome, summary, notes, issues, followup_session_id, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    (
                        note.id.as_str(),
                        note.task_id.as_str(),
                        note.reviewer.to_string(),
                        note.outcome.to_string(),
                        note.summary.as_deref(),
                        note.notes.as_deref(),
                        issues_json.as_deref(),
                        note.followup_session_id.as_deref(),
                        SqliteReviewRepository::format_datetime(&note.created_at),
                    ),
                )
                .map_err(|e| AppError::Database(format!("Failed to create review note: {}", e)))?;
                Ok(())
            })
            .await
    }

    async fn get_notes_by_task_id(&self, task_id: &TaskId) -> AppResult<Vec<ReviewNote>> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, task_id, reviewer, outcome, summary, notes, issues, followup_session_id, created_at
                     FROM review_notes WHERE task_id = ?1 ORDER BY created_at ASC",
                )
                .map_err(|e| AppError::Database(format!("Failed to prepare statement: {}", e)))?;

                let rows: Vec<(String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, String)> = stmt
                    .query_map([task_id.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, Option<String>>(7)?,
                            row.get::<_, String>(8)?,
                        ))
                    })
                    .map_err(|e| AppError::Database(format!("Failed to query notes: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::Database(format!("Failed to read row: {}", e)))?;

                let mut notes = Vec::new();
                for (id, task_id, reviewer, outcome, summary, note_text, issues_json, followup_session_id, created_at) in rows {
                    notes.push(SqliteReviewRepository::row_to_note(
                        id,
                        task_id,
                        reviewer,
                        outcome,
                        summary,
                        note_text,
                        issues_json,
                        followup_session_id,
                        created_at,
                    )?);
                }
                Ok(notes)
            })
            .await
    }

    async fn get_note_by_id(&self, id: &ReviewNoteId) -> AppResult<Option<ReviewNote>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT id, task_id, reviewer, outcome, summary, notes, issues, followup_session_id, created_at
                     FROM review_notes WHERE id = ?1",
                    [id.as_str()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, Option<String>>(7)?,
                            row.get::<_, String>(8)?,
                        ))
                    },
                );

                match result {
                    Ok((id, task_id, reviewer, outcome, summary, note_text, issues_json, followup_session_id, created_at)) => {
                        Ok(Some(SqliteReviewRepository::row_to_note(
                            id,
                            task_id,
                            reviewer,
                            outcome,
                            summary,
                            note_text,
                            issues_json,
                            followup_session_id,
                            created_at,
                        )?))
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(format!("Failed to get note: {}", e))),
                }
            })
            .await
    }

    // ========================================
    // Query methods
    // ========================================

    async fn get_by_status(
        &self,
        project_id: &ProjectId,
        status: ReviewStatus,
    ) -> AppResult<Vec<Review>> {
        let project_id = project_id.as_str().to_string();
        let status_str = status.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, task_id, reviewer_type, status, notes, created_at, completed_at
                     FROM reviews WHERE project_id = ?1 AND status = ?2 ORDER BY created_at DESC",
                )
                .map_err(|e| AppError::Database(format!("Failed to prepare statement: {}", e)))?;

                let rows: Vec<(String, String, String, String, String, Option<String>, String, Option<String>)> = stmt
                    .query_map([project_id.as_str(), status_str.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, Option<String>>(7)?,
                        ))
                    })
                    .map_err(|e| AppError::Database(format!("Failed to query reviews: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::Database(format!("Failed to read row: {}", e)))?;

                let mut reviews = Vec::new();
                for (id, project_id, task_id, reviewer_type, status, notes, created_at, completed_at) in rows {
                    reviews.push(SqliteReviewRepository::row_to_review(
                        id,
                        project_id,
                        task_id,
                        reviewer_type,
                        status,
                        notes,
                        created_at,
                        completed_at,
                    )?);
                }
                Ok(reviews)
            })
            .await
    }

    async fn count_pending(&self, project_id: &ProjectId) -> AppResult<u32> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM reviews WHERE project_id = ?1 AND status = 'pending'",
                        [project_id.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(|e| AppError::Database(format!("Failed to count reviews: {}", e)))?;
                Ok(count as u32)
            })
            .await
    }

    async fn has_pending_review(&self, task_id: &TaskId) -> AppResult<bool> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM reviews WHERE task_id = ?1 AND status = 'pending'",
                        [task_id.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(|e| AppError::Database(format!("Failed to check pending: {}", e)))?;
                Ok(count > 0)
            })
            .await
    }

    async fn count_fix_actions(&self, task_id: &TaskId) -> AppResult<u32> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*)
                         FROM review_actions ra
                         INNER JOIN reviews r ON ra.review_id = r.id
                         WHERE r.task_id = ?1 AND ra.action_type = 'created_fix_task'",
                        [task_id.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(|e| {
                        AppError::Database(format!("Failed to count fix actions: {}", e))
                    })?;
                Ok(count as u32)
            })
            .await
    }

    async fn get_fix_actions(&self, task_id: &TaskId) -> AppResult<Vec<ReviewAction>> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT ra.id, ra.review_id, ra.action_type, ra.target_task_id, ra.created_at
                     FROM review_actions ra
                     INNER JOIN reviews r ON ra.review_id = r.id
                     WHERE r.task_id = ?1 AND ra.action_type = 'created_fix_task'
                     ORDER BY ra.created_at ASC",
                )
                .map_err(|e| AppError::Database(format!("Failed to prepare statement: {}", e)))?;

                let rows: Vec<(String, String, String, Option<String>, String)> = stmt
                    .query_map([task_id.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    })
                    .map_err(|e| {
                        AppError::Database(format!("Failed to query fix actions: {}", e))
                    })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::Database(format!("Failed to read row: {}", e)))?;

                let mut actions = Vec::new();
                for (id, review_id, action_type, target_task_id, created_at) in rows {
                    actions.push(SqliteReviewRepository::row_to_action(
                        id,
                        review_id,
                        action_type,
                        target_task_id,
                        created_at,
                    )?);
                }
                Ok(actions)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_review_repo_tests.rs"]
mod tests;
