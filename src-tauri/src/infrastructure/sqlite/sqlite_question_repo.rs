use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;
use serde_json::Value;

use super::DbConnection;
use crate::application::question_state::{PendingQuestionInfo, QuestionAnswer, QuestionOption};
use crate::domain::repositories::question_repository::QuestionRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteQuestionRepository {
    db: DbConnection,
}

impl SqliteQuestionRepository {
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

fn parse_metadata_json(metadata_json: Option<String>) -> AppResult<Option<Value>> {
    metadata_json
        .map(|json| serde_json::from_str(&json).map_err(|e| AppError::Database(e.to_string())))
        .transpose()
}

#[async_trait]
impl QuestionRepository for SqliteQuestionRepository {
    async fn create_pending(&self, info: &PendingQuestionInfo) -> AppResult<()> {
        let options_json =
            serde_json::to_string(&info.options).map_err(|e| AppError::Database(e.to_string()))?;
        let request_id = info.request_id.clone();
        let session_id = info.session_id.clone();
        let question = info.question.clone();
        let header = info.header.clone();
        let multi_select = info.multi_select;
        let allow_skip = info.allow_skip;
        let batch_index = info.batch_index.map(i64::from);
        let batch_total = info.batch_total.map(i64::from);
        let metadata_json = info.metadata.as_ref().map(ToString::to_string);
        let created_at = info.created_at.clone();

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO pending_questions (
                        request_id,
                        session_id,
                        question,
                        header,
                        options,
                        multi_select,
                        allow_skip,
                        batch_index,
                        batch_total,
                        metadata,
                        status,
                        created_at
                     )
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'pending', ?11)",
                    rusqlite::params![
                        request_id,
                        session_id,
                        question,
                        header,
                        options_json,
                        multi_select as i64,
                        allow_skip as i64,
                        batch_index,
                        batch_total,
                        metadata_json,
                        created_at,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn resolve(&self, request_id: &str, answer: &QuestionAnswer) -> AppResult<bool> {
        let selected_json = serde_json::to_string(&answer.selected_options)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let request_id = request_id.to_string();
        let answer_text = answer.text.clone();
        let skipped = answer.skipped;

        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "UPDATE pending_questions
                     SET status = 'resolved',
                         answer_selected_options = ?1,
                         answer_text = ?2,
                         answer_skipped = ?3,
                         resolved_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     WHERE request_id = ?4 AND status IN ('pending', 'wait_expired')",
                    rusqlite::params![selected_json, answer_text, skipped as i64, request_id],
                )?;
                Ok(rows > 0)
            })
            .await
    }

    async fn get_pending(&self) -> AppResult<Vec<PendingQuestionInfo>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT request_id,
                            session_id,
                            question,
                            header,
                            options,
                            multi_select,
                            allow_skip,
                            batch_index,
                            batch_total,
                            metadata,
                            created_at
                     FROM pending_questions
                     WHERE status IN ('pending', 'wait_expired')",
                )?;

                let mapped_rows = stmt.query_map([], |row| {
                    let options_json: String = row.get(4)?;
                    let multi_select_int: i64 = row.get(5)?;
                    let allow_skip_int: i64 = row.get(6)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        options_json,
                        multi_select_int,
                        allow_skip_int,
                        row.get::<_, Option<i64>>(7)?,
                        row.get::<_, Option<i64>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, String>(10)?,
                    ))
                })?;

                let mut results = Vec::new();
                for row_result in mapped_rows {
                    let (
                        request_id,
                        session_id,
                        question,
                        header,
                        options_json,
                        multi_select_int,
                        allow_skip_int,
                        batch_index,
                        batch_total,
                        metadata_json,
                        created_at,
                    ) = row_result?;
                    let options: Vec<QuestionOption> = serde_json::from_str(&options_json)
                        .map_err(|e| AppError::Database(e.to_string()))?;
                    let metadata = parse_metadata_json(metadata_json)?;
                    results.push(PendingQuestionInfo {
                        request_id,
                        session_id,
                        question,
                        header,
                        options,
                        multi_select: multi_select_int != 0,
                        allow_skip: allow_skip_int != 0,
                        batch_index: batch_index.and_then(|value| u32::try_from(value).ok()),
                        batch_total: batch_total.and_then(|value| u32::try_from(value).ok()),
                        metadata,
                        created_at,
                    });
                }

                Ok(results)
            })
            .await
    }

    async fn get_by_request_id(&self, request_id: &str) -> AppResult<Option<PendingQuestionInfo>> {
        let request_id = request_id.to_string();
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT request_id,
                            session_id,
                            question,
                            header,
                            options,
                            multi_select,
                            allow_skip,
                            batch_index,
                            batch_total,
                            metadata,
                            created_at
                     FROM pending_questions WHERE request_id = ?1",
                    rusqlite::params![request_id],
                    |row| {
                        let options_json: String = row.get(4)?;
                        let multi_select_int: i64 = row.get(5)?;
                        let allow_skip_int: i64 = row.get(6)?;
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            options_json,
                            multi_select_int,
                            allow_skip_int,
                            row.get::<_, Option<i64>>(7)?,
                            row.get::<_, Option<i64>>(8)?,
                            row.get::<_, Option<String>>(9)?,
                            row.get::<_, String>(10)?,
                        ))
                    },
                );

                match result {
                    Ok((
                        request_id,
                        session_id,
                        question,
                        header,
                        options_json,
                        multi_select_int,
                        allow_skip_int,
                        batch_index,
                        batch_total,
                        metadata_json,
                        created_at,
                    )) => {
                        let options: Vec<QuestionOption> = serde_json::from_str(&options_json)
                            .map_err(|e| AppError::Database(e.to_string()))?;
                        let metadata = parse_metadata_json(metadata_json)?;
                        Ok(Some(PendingQuestionInfo {
                            request_id,
                            session_id,
                            question,
                            header,
                            options,
                            multi_select: multi_select_int != 0,
                            allow_skip: allow_skip_int != 0,
                            batch_index: batch_index.and_then(|value| u32::try_from(value).ok()),
                            batch_total: batch_total.and_then(|value| u32::try_from(value).ok()),
                            metadata,
                            created_at,
                        }))
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(e.to_string())),
                }
            })
            .await
    }

    async fn expire_all_pending(&self) -> AppResult<u64> {
        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "UPDATE pending_questions
                     SET status = 'wait_expired',
                         resolved_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     WHERE status = 'pending'",
                    [],
                )?;
                Ok(rows as u64)
            })
            .await
    }

    async fn expire_by_request_id(&self, request_id: &str) -> AppResult<()> {
        let request_id = request_id.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE pending_questions
                     SET status = 'wait_expired',
                         resolved_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     WHERE request_id = ?1 AND status = 'pending'",
                    rusqlite::params![request_id],
                )?;
                Ok(())
            })
            .await
    }

    async fn remove(&self, request_id: &str) -> AppResult<bool> {
        let request_id = request_id.to_string();
        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "DELETE FROM pending_questions WHERE request_id = ?1",
                    rusqlite::params![request_id],
                )?;
                Ok(rows > 0)
            })
            .await
    }

    async fn get_resolved_answer(&self, request_id: &str) -> AppResult<Option<QuestionAnswer>> {
        let request_id = request_id.to_string();
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT answer_selected_options, answer_text, answer_skipped
                     FROM pending_questions WHERE request_id = ?1 AND status = 'resolved'",
                    rusqlite::params![request_id],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                );

                match result {
                    Ok((selected_json, answer_text, skipped_int)) => {
                        let selected_options = match selected_json {
                            Some(json) => serde_json::from_str::<Vec<String>>(&json)
                                .map_err(|e| AppError::Database(e.to_string()))?,
                            None => vec![],
                        };
                        Ok(Some(QuestionAnswer {
                            selected_options,
                            text: answer_text,
                            skipped: skipped_int != 0,
                        }))
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(e.to_string())),
                }
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_question_repo_tests.rs"]
mod tests;
