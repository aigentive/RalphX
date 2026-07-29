// SQLite-based ActivityEventRepository implementation for production use
// Uses rusqlite with connection pooling for thread-safe access
// Implements cursor-based pagination using (created_at, id) tuples
// Cursor format: "timestamp|id" with pipe separator to avoid ISO 8601 colon conflicts

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use super::DbConnection;
use crate::domain::entities::{ActivityEvent, ActivityEventId, IdeationSessionId, TaskId};
use crate::domain::repositories::{
    ActivityEventFilter, ActivityEventPage, ActivityEventRepository,
};
use crate::error::{AppError, AppResult};

/// Maximum allowed limit for pagination
const MAX_LIMIT: u32 = 100;

/// SQLite implementation of ActivityEventRepository for production use
/// Uses a mutex-protected connection for thread-safe access
pub struct SqliteActivityEventRepository {
    pub(crate) db: DbConnection,
}

impl SqliteActivityEventRepository {
    /// Create a new SQLite activity event repository with the given connection
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

    /// Parse a cursor string into (timestamp, id) tuple
    /// Cursor format: "timestamp|id" using pipe separator to avoid ISO 8601 colon conflicts
    fn parse_cursor(cursor: &str) -> Option<(String, String)> {
        cursor
            .split_once('|')
            .map(|(ts, id)| (ts.to_string(), id.to_string()))
    }

    /// Build filter clause with positional placeholders starting at the given index
    /// Does NOT include task_id/session_id filtering (those are handled by caller)
    fn build_filter_clause(
        filter: Option<&ActivityEventFilter>,
        start_param_idx: usize,
    ) -> (String, Vec<String>) {
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        let mut idx = start_param_idx;

        if let Some(f) = filter {
            if let Some(event_types) = &f.event_types {
                if !event_types.is_empty() {
                    let placeholders: Vec<String> = event_types
                        .iter()
                        .map(|t| {
                            params.push(t.to_string());
                            let placeholder = format!("?{}", idx);
                            idx += 1;
                            placeholder
                        })
                        .collect();
                    conditions.push(format!("event_type IN ({})", placeholders.join(", ")));
                }
            }
            if let Some(roles) = &f.roles {
                if !roles.is_empty() {
                    let placeholders: Vec<String> = roles
                        .iter()
                        .map(|r| {
                            params.push(r.to_string());
                            let placeholder = format!("?{}", idx);
                            idx += 1;
                            placeholder
                        })
                        .collect();
                    conditions.push(format!("role IN ({})", placeholders.join(", ")));
                }
            }
            if let Some(statuses) = &f.statuses {
                if !statuses.is_empty() {
                    let placeholders: Vec<String> = statuses
                        .iter()
                        .map(|s| {
                            params.push(s.to_string());
                            let placeholder = format!("?{}", idx);
                            idx += 1;
                            placeholder
                        })
                        .collect();
                    conditions.push(format!("internal_status IN ({})", placeholders.join(", ")));
                }
            }
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" AND {}", conditions.join(" AND "))
        };

        (where_clause, params)
    }

    /// Format a cursor from an event
    pub(crate) fn format_cursor(event: &ActivityEvent) -> String {
        format!("{}|{}", event.created_at.to_rfc3339(), event.id)
    }

    /// Build filter clause for list_all that includes task_id/session_id filtering
    fn build_list_all_filter_clause(
        filter: Option<&ActivityEventFilter>,
        start_param_idx: usize,
    ) -> (Vec<String>, Vec<String>) {
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        let mut idx = start_param_idx;

        if let Some(f) = filter {
            if let Some(task_id) = &f.task_id {
                conditions.push(format!("task_id = ?{}", idx));
                params.push(task_id.as_str().to_string());
                idx += 1;
            }

            if let Some(session_id) = &f.session_id {
                conditions.push(format!("ideation_session_id = ?{}", idx));
                params.push(session_id.as_str().to_string());
                idx += 1;
            }

            if let Some(event_types) = &f.event_types {
                if !event_types.is_empty() {
                    let placeholders: Vec<String> = event_types
                        .iter()
                        .map(|t| {
                            params.push(t.to_string());
                            let placeholder = format!("?{}", idx);
                            idx += 1;
                            placeholder
                        })
                        .collect();
                    conditions.push(format!("event_type IN ({})", placeholders.join(", ")));
                }
            }

            if let Some(roles) = &f.roles {
                if !roles.is_empty() {
                    let placeholders: Vec<String> = roles
                        .iter()
                        .map(|r| {
                            params.push(r.to_string());
                            let placeholder = format!("?{}", idx);
                            idx += 1;
                            placeholder
                        })
                        .collect();
                    conditions.push(format!("role IN ({})", placeholders.join(", ")));
                }
            }

            if let Some(statuses) = &f.statuses {
                if !statuses.is_empty() {
                    let placeholders: Vec<String> = statuses
                        .iter()
                        .map(|s| {
                            params.push(s.to_string());
                            let placeholder = format!("?{}", idx);
                            idx += 1;
                            placeholder
                        })
                        .collect();
                    conditions.push(format!("internal_status IN ({})", placeholders.join(", ")));
                }
            }
        }

        (conditions, params)
    }
}

#[async_trait]
impl ActivityEventRepository for SqliteActivityEventRepository {
    async fn save(&self, event: ActivityEvent) -> AppResult<ActivityEvent> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO activity_events (id, task_id, ideation_session_id, internal_status, event_type, role, content, metadata, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    rusqlite::params![
                        event.id.as_str(),
                        event.task_id.as_ref().map(|id| id.as_str()),
                        event.ideation_session_id.as_ref().map(|id| id.as_str()),
                        event.internal_status.as_ref().map(|s| s.to_string()),
                        event.event_type.to_string(),
                        event.role.to_string(),
                        event.content,
                        event.metadata,
                        event.created_at.to_rfc3339(),
                    ],
                )?;
                Ok(event)
            })
            .await
    }

    async fn get_by_id(&self, id: &ActivityEventId) -> AppResult<Option<ActivityEvent>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT id, task_id, ideation_session_id, internal_status, event_type, role, content, metadata, created_at
                     FROM activity_events WHERE id = ?1",
                    [id.as_str()],
                    ActivityEvent::from_row,
                );
                match result {
                    Ok(event) => Ok(Some(event)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(e.to_string())),
                }
            })
            .await
    }

    async fn list_by_task_id(
        &self,
        task_id: &TaskId,
        cursor: Option<&str>,
        limit: u32,
        filter: Option<&ActivityEventFilter>,
    ) -> AppResult<ActivityEventPage> {
        let limit = limit.min(MAX_LIMIT);
        let fetch_limit = limit + 1;
        let task_id_str = task_id.as_str().to_string();
        let cursor_owned = cursor.map(|s| s.to_string());

        let (sql, params): (String, Vec<String>) = if let Some(ref cursor_str) = cursor_owned {
            if let Some((cursor_ts, cursor_id)) = Self::parse_cursor(cursor_str) {
                let (filter_clause, filter_params) = Self::build_filter_clause(filter, 5);
                let sql = format!(
                    "SELECT id, task_id, ideation_session_id, internal_status, event_type, role, content, metadata, created_at
                     FROM activity_events
                     WHERE task_id = ?1 AND (created_at < ?2 OR (created_at = ?2 AND id < ?3)){}
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?4",
                    filter_clause
                );
                let mut params = vec![
                    task_id_str,
                    cursor_ts,
                    cursor_id,
                    fetch_limit.to_string(),
                ];
                params.extend(filter_params);
                (sql, params)
            } else {
                let (filter_clause, filter_params) = Self::build_filter_clause(filter, 3);
                let sql = format!(
                    "SELECT id, task_id, ideation_session_id, internal_status, event_type, role, content, metadata, created_at
                     FROM activity_events
                     WHERE task_id = ?1{}
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?2",
                    filter_clause
                );
                let mut params = vec![task_id_str, fetch_limit.to_string()];
                params.extend(filter_params);
                (sql, params)
            }
        } else {
            let (filter_clause, filter_params) = Self::build_filter_clause(filter, 3);
            let sql = format!(
                "SELECT id, task_id, ideation_session_id, internal_status, event_type, role, content, metadata, created_at
                 FROM activity_events
                 WHERE task_id = ?1{}
                 ORDER BY created_at DESC, id DESC
                 LIMIT ?2",
                filter_clause
            );
            let mut params = vec![task_id_str, fetch_limit.to_string()];
            params.extend(filter_params);
            (sql, params)
        };

        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| AppError::Database(e.to_string()))?;
                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
                let mut events: Vec<ActivityEvent> = stmt
                    .query_map(params_refs.as_slice(), ActivityEvent::from_row)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::Database(e.to_string()))?;

                let has_more = events.len() > limit as usize;
                if has_more {
                    events.truncate(limit as usize);
                }
                let next_cursor = if has_more {
                    events
                        .last()
                        .map(SqliteActivityEventRepository::format_cursor)
                } else {
                    None
                };
                Ok(ActivityEventPage {
                    events,
                    cursor: next_cursor,
                    has_more,
                })
            })
            .await
    }

    async fn list_by_session_id(
        &self,
        session_id: &IdeationSessionId,
        cursor: Option<&str>,
        limit: u32,
        filter: Option<&ActivityEventFilter>,
    ) -> AppResult<ActivityEventPage> {
        let limit = limit.min(MAX_LIMIT);
        let fetch_limit = limit + 1;
        let session_id_str = session_id.as_str().to_string();
        let cursor_owned = cursor.map(|s| s.to_string());

        let (sql, params): (String, Vec<String>) = if let Some(ref cursor_str) = cursor_owned {
            if let Some((cursor_ts, cursor_id)) = Self::parse_cursor(cursor_str) {
                let (filter_clause, filter_params) = Self::build_filter_clause(filter, 5);
                let sql = format!(
                    "SELECT id, task_id, ideation_session_id, internal_status, event_type, role, content, metadata, created_at
                     FROM activity_events
                     WHERE ideation_session_id = ?1 AND (created_at < ?2 OR (created_at = ?2 AND id < ?3)){}
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?4",
                    filter_clause
                );
                let mut params = vec![
                    session_id_str,
                    cursor_ts,
                    cursor_id,
                    fetch_limit.to_string(),
                ];
                params.extend(filter_params);
                (sql, params)
            } else {
                let (filter_clause, filter_params) = Self::build_filter_clause(filter, 3);
                let sql = format!(
                    "SELECT id, task_id, ideation_session_id, internal_status, event_type, role, content, metadata, created_at
                     FROM activity_events
                     WHERE ideation_session_id = ?1{}
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?2",
                    filter_clause
                );
                let mut params = vec![session_id_str, fetch_limit.to_string()];
                params.extend(filter_params);
                (sql, params)
            }
        } else {
            let (filter_clause, filter_params) = Self::build_filter_clause(filter, 3);
            let sql = format!(
                "SELECT id, task_id, ideation_session_id, internal_status, event_type, role, content, metadata, created_at
                 FROM activity_events
                 WHERE ideation_session_id = ?1{}
                 ORDER BY created_at DESC, id DESC
                 LIMIT ?2",
                filter_clause
            );
            let mut params = vec![session_id_str, fetch_limit.to_string()];
            params.extend(filter_params);
            (sql, params)
        };

        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| AppError::Database(e.to_string()))?;
                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
                let mut events: Vec<ActivityEvent> = stmt
                    .query_map(params_refs.as_slice(), ActivityEvent::from_row)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::Database(e.to_string()))?;

                let has_more = events.len() > limit as usize;
                if has_more {
                    events.truncate(limit as usize);
                }
                let next_cursor = if has_more {
                    events
                        .last()
                        .map(SqliteActivityEventRepository::format_cursor)
                } else {
                    None
                };
                Ok(ActivityEventPage {
                    events,
                    cursor: next_cursor,
                    has_more,
                })
            })
            .await
    }

    async fn delete_by_task_id(&self, task_id: &TaskId) -> AppResult<()> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM activity_events WHERE task_id = ?1",
                    [task_id.as_str()],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete_by_session_id(&self, session_id: &IdeationSessionId) -> AppResult<()> {
        let session_id = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM activity_events WHERE ideation_session_id = ?1",
                    [session_id.as_str()],
                )?;
                Ok(())
            })
            .await
    }

    async fn count_by_task_id(
        &self,
        task_id: &TaskId,
        filter: Option<&ActivityEventFilter>,
    ) -> AppResult<u64> {
        let (filter_clause, filter_params) = Self::build_filter_clause(filter, 2);
        let task_id_str = task_id.as_str().to_string();
        let sql = format!(
            "SELECT COUNT(*) FROM activity_events WHERE task_id = ?1{}",
            filter_clause
        );
        let mut params = vec![task_id_str];
        params.extend(filter_params);

        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| AppError::Database(e.to_string()))?;
                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
                let count: i64 = stmt
                    .query_row(params_refs.as_slice(), |row| row.get(0))
                    .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(count as u64)
            })
            .await
    }

    async fn count_by_session_id(
        &self,
        session_id: &IdeationSessionId,
        filter: Option<&ActivityEventFilter>,
    ) -> AppResult<u64> {
        let (filter_clause, filter_params) = Self::build_filter_clause(filter, 2);
        let session_id_str = session_id.as_str().to_string();
        let sql = format!(
            "SELECT COUNT(*) FROM activity_events WHERE ideation_session_id = ?1{}",
            filter_clause
        );
        let mut params = vec![session_id_str];
        params.extend(filter_params);

        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| AppError::Database(e.to_string()))?;
                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
                let count: i64 = stmt
                    .query_row(params_refs.as_slice(), |row| row.get(0))
                    .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(count as u64)
            })
            .await
    }

    async fn list_all(
        &self,
        cursor: Option<&str>,
        limit: u32,
        filter: Option<&ActivityEventFilter>,
    ) -> AppResult<ActivityEventPage> {
        let limit = limit.min(MAX_LIMIT);
        let fetch_limit = limit + 1;
        let cursor_owned = cursor.map(|s| s.to_string());

        let (sql, params): (String, Vec<String>) = if let Some(ref cursor_str) = cursor_owned {
            if let Some((cursor_ts, cursor_id)) = Self::parse_cursor(cursor_str) {
                let (filter_conditions, filter_params) =
                    Self::build_list_all_filter_clause(filter, 4);
                let where_clause = if filter_conditions.is_empty() {
                    "(created_at < ?1 OR (created_at = ?1 AND id < ?2))".to_string()
                } else {
                    format!(
                        "(created_at < ?1 OR (created_at = ?1 AND id < ?2)) AND {}",
                        filter_conditions.join(" AND ")
                    )
                };
                let sql = format!(
                    "SELECT id, task_id, ideation_session_id, internal_status, event_type, role, content, metadata, created_at
                     FROM activity_events
                     WHERE {}
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?3",
                    where_clause
                );
                let mut params = vec![cursor_ts, cursor_id, fetch_limit.to_string()];
                params.extend(filter_params);
                (sql, params)
            } else {
                let (filter_conditions, filter_params) =
                    Self::build_list_all_filter_clause(filter, 2);
                let where_clause = if filter_conditions.is_empty() {
                    "1=1".to_string()
                } else {
                    filter_conditions.join(" AND ")
                };
                let sql = format!(
                    "SELECT id, task_id, ideation_session_id, internal_status, event_type, role, content, metadata, created_at
                     FROM activity_events
                     WHERE {}
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?1",
                    where_clause
                );
                let mut params = vec![fetch_limit.to_string()];
                params.extend(filter_params);
                (sql, params)
            }
        } else {
            let (filter_conditions, filter_params) = Self::build_list_all_filter_clause(filter, 2);
            let where_clause = if filter_conditions.is_empty() {
                "1=1".to_string()
            } else {
                filter_conditions.join(" AND ")
            };
            let sql = format!(
                "SELECT id, task_id, ideation_session_id, internal_status, event_type, role, content, metadata, created_at
                 FROM activity_events
                 WHERE {}
                 ORDER BY created_at DESC, id DESC
                 LIMIT ?1",
                where_clause
            );
            let mut params = vec![fetch_limit.to_string()];
            params.extend(filter_params);
            (sql, params)
        };

        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| AppError::Database(e.to_string()))?;
                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
                let mut events: Vec<ActivityEvent> = stmt
                    .query_map(params_refs.as_slice(), ActivityEvent::from_row)
                    .map_err(|e| AppError::Database(e.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::Database(e.to_string()))?;

                let has_more = events.len() > limit as usize;
                if has_more {
                    events.truncate(limit as usize);
                }
                let next_cursor = if has_more {
                    events
                        .last()
                        .map(SqliteActivityEventRepository::format_cursor)
                } else {
                    None
                };
                Ok(ActivityEventPage {
                    events,
                    cursor: next_cursor,
                    has_more,
                })
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_activity_event_repo_tests.rs"]
mod tests;
