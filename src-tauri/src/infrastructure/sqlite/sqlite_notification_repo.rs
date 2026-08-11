use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::entities::{Notification, NotificationTarget};
use crate::domain::repositories::{NotificationPage, NotificationRepository};
use crate::error::{AppError, AppResult};

const MAX_LIMIT: u32 = 100;
const RETENTION_PROTECTED_PREDICATE: &str = "category = 'plan_approval' AND read_at IS NULL";
const VISIBLE_NOTIFICATION_PREDICATE: &str = r#"
    NOT EXISTS (
        SELECT 1
        FROM chat_conversations AS conversation
        WHERE conversation.archived_at IS NOT NULL
          AND conversation.id IN (
              json_extract(
                  CASE WHEN json_valid(notifications.target_json) THEN notifications.target_json END,
                  '$.conversationId'
              ),
              json_extract(
                  CASE WHEN json_valid(notifications.target_json) THEN notifications.target_json END,
                  '$.setupConversationId'
              )
          )
    )
    AND NOT EXISTS (
        SELECT 1
        FROM agent_conversation_workspaces AS workspace
        WHERE workspace.status = 'archived'
          AND workspace.conversation_id IN (
              json_extract(
                  CASE WHEN json_valid(notifications.target_json) THEN notifications.target_json END,
                  '$.conversationId'
              ),
              json_extract(
                  CASE WHEN json_valid(notifications.target_json) THEN notifications.target_json END,
                  '$.setupConversationId'
              )
          )
    )
"#;

pub struct SqliteNotificationRepository {
    db: DbConnection,
}

impl SqliteNotificationRepository {
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

    fn cursor(notification: &Notification) -> String {
        format!(
            "{}|{}",
            notification.created_at.to_rfc3339(),
            notification.id
        )
    }

    fn parse_cursor(cursor: &str) -> Option<(String, String)> {
        cursor
            .split_once('|')
            .map(|(created_at, id)| (created_at.to_owned(), id.to_owned()))
    }

    fn enum_string<T: serde::Serialize>(value: &T) -> AppResult<String> {
        serde_json::to_value(value)
            .map_err(|error| AppError::Infrastructure(error.to_string()))?
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                AppError::Infrastructure("notification enum did not serialize to a string".into())
            })
    }

    fn parse_row(
        row: (
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    ) -> AppResult<Notification> {
        let (
            id,
            created_at,
            project_id,
            category,
            severity,
            title,
            body,
            target_json,
            dedupe_key,
            read_at,
        ) = row;
        Ok(Notification {
            id,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map_err(|error| {
                    AppError::Database(format!("invalid notification created_at: {error}"))
                })?
                .with_timezone(&Utc),
            project_id,
            category: serde_json::from_value(serde_json::Value::String(category)).map_err(
                |error| AppError::Database(format!("invalid notification category: {error}")),
            )?,
            severity: serde_json::from_value(serde_json::Value::String(severity)).map_err(
                |error| AppError::Database(format!("invalid notification severity: {error}")),
            )?,
            title,
            body,
            target: match target_json {
                Some(json) => serde_json::from_str(&json).map_err(|error| {
                    AppError::Database(format!("invalid notification target: {error}"))
                })?,
                None => NotificationTarget::none(),
            },
            dedupe_key,
            read_at: read_at
                .map(|value| {
                    DateTime::parse_from_rfc3339(&value)
                        .map(|date| date.with_timezone(&Utc))
                        .map_err(|error| {
                            AppError::Database(format!("invalid notification read_at: {error}"))
                        })
                })
                .transpose()?,
        })
    }

    fn select_columns() -> &'static str {
        "id, created_at, project_id, category, severity, title, body, target_json, dedupe_key, read_at"
    }
}

#[async_trait]
impl NotificationRepository for SqliteNotificationRepository {
    async fn create_with_dedupe(&self, notification: Notification) -> AppResult<bool> {
        self.db.run(move |conn| {
            let category = Self::enum_string(&notification.category)?;
            let severity = Self::enum_string(&notification.severity)?;
            let target = serde_json::to_string(&notification.target)
                .map_err(|error| AppError::Infrastructure(error.to_string()))?;
            Ok(conn.execute(
                "INSERT INTO notifications (id, created_at, project_id, category, severity, title, body, target_json, dedupe_key, read_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(dedupe_key) DO NOTHING",
                params![notification.id, notification.created_at.to_rfc3339(), notification.project_id,
                    category, severity, notification.title, notification.body, target,
                    notification.dedupe_key, notification.read_at.map(|value| value.to_rfc3339())],
            )? == 1)
        }).await
    }

    async fn list(
        &self,
        project_id: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> AppResult<NotificationPage> {
        let project_id = project_id.map(str::to_owned);
        let cursor = cursor.and_then(Self::parse_cursor);
        let limit = limit.clamp(1, MAX_LIMIT);
        self.db.run(move |conn| {
            let mut where_parts = vec![VISIBLE_NOTIFICATION_PREDICATE.to_string()];
            let mut values = Vec::new();
            if let Some(project_id) = project_id {
                where_parts.push("project_id = ?".to_string()); values.push(project_id);
            }
            if let Some((created_at, id)) = cursor {
                where_parts.push("(created_at < ? OR (created_at = ? AND id < ?))".to_string());
                values.extend([created_at.clone(), created_at, id]);
            }
            values.push((limit + 1).to_string());
            let where_clause = if where_parts.is_empty() { String::new() } else { format!(" WHERE {}", where_parts.join(" AND ")) };
            let sql = format!("SELECT {} FROM notifications{} ORDER BY created_at DESC, id DESC LIMIT ?", Self::select_columns(), where_clause);
            let mut statement = conn.prepare(&sql)?;
            let rows = statement.query_map(rusqlite::params_from_iter(values), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?))
            })?;
            let mut notifications = Vec::new();
            for row in rows {
                match Self::parse_row(row?) {
                    Ok(notification) => notifications.push(notification),
                    Err(error) => tracing::warn!(error = %error, "Skipping malformed durable notification row"),
                }
            }
            let has_more = notifications.len() > limit as usize;
            notifications.truncate(limit as usize);
            let cursor = has_more.then(|| notifications.last().map(Self::cursor)).flatten();
            Ok(NotificationPage { notifications, cursor, has_more })
        }).await
    }

    async fn unread_count(&self, project_id: Option<&str>) -> AppResult<u64> {
        let project_id = project_id.map(str::to_owned);
        self.db.run(move |conn| {
            let count: i64 = match project_id {
                Some(project_id) => conn.query_row(
                    &format!("SELECT COUNT(*) FROM notifications WHERE read_at IS NULL AND project_id = ?1 AND {VISIBLE_NOTIFICATION_PREDICATE}"),
                    [project_id],
                    |row| row.get(0),
                )?,
                None => conn.query_row(
                    &format!("SELECT COUNT(*) FROM notifications WHERE read_at IS NULL AND {VISIBLE_NOTIFICATION_PREDICATE}"),
                    [],
                    |row| row.get(0),
                )?,
            };
            Ok(count as u64)
        }).await
    }

    async fn mark_read(&self, id: &str, read_at: DateTime<Utc>) -> AppResult<Option<Notification>> {
        let id = id.to_owned();
        self.db
            .run(move |conn| {
                if conn.execute(
                    "UPDATE notifications SET read_at = ?1 WHERE id = ?2 AND read_at IS NULL",
                    params![read_at.to_rfc3339(), id],
                )? == 0
                {
                    return Ok(None);
                }
                let raw = conn.query_row(
                    &format!(
                        "SELECT {} FROM notifications WHERE id = ?1",
                        Self::select_columns()
                    ),
                    [id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                            row.get(8)?,
                            row.get(9)?,
                        ))
                    },
                )?;
                Self::parse_row(raw).map(Some)
            })
            .await
    }

    async fn mark_read_by_dedupe_key(
        &self,
        dedupe_key: &str,
        read_at: DateTime<Utc>,
    ) -> AppResult<Option<Notification>> {
        let dedupe_key = dedupe_key.to_owned();
        self.db
            .run(move |conn| {
                if conn.execute(
                    "UPDATE notifications SET read_at = ?1 WHERE dedupe_key = ?2 AND read_at IS NULL",
                    params![read_at.to_rfc3339(), dedupe_key],
                )? == 0
                {
                    return Ok(None);
                }
                let raw = conn.query_row(
                    &format!(
                        "SELECT {} FROM notifications WHERE dedupe_key = ?1",
                        Self::select_columns()
                    ),
                    [dedupe_key],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                            row.get(8)?,
                            row.get(9)?,
                        ))
                    },
                )?;
                Self::parse_row(raw).map(Some)
            })
            .await
    }

    async fn mark_all_read(
        &self,
        project_id: Option<&str>,
        read_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let project_id = project_id.map(str::to_owned);
        self.db.run(move |conn| {
            let changed = match project_id {
                Some(project_id) => conn.execute(
                    &format!("UPDATE notifications SET read_at = ?1 WHERE read_at IS NULL AND project_id = ?2 AND {VISIBLE_NOTIFICATION_PREDICATE}"),
                    params![read_at.to_rfc3339(), project_id],
                )?,
                None => conn.execute(
                    &format!("UPDATE notifications SET read_at = ?1 WHERE read_at IS NULL AND {VISIBLE_NOTIFICATION_PREDICATE}"),
                    [read_at.to_rfc3339()],
                )?,
            };
            Ok(changed as u64)
        }).await
    }

    async fn prune(&self, read_before: DateTime<Utc>, max_rows: u32) -> AppResult<()> {
        self.db
            .run(move |conn| {
                conn.execute(
                    &format!(
                        "DELETE FROM notifications
                     WHERE read_at IS NOT NULL AND read_at < ?1
                       AND NOT ({RETENTION_PROTECTED_PREDICATE})"
                    ),
                    [read_before.to_rfc3339()],
                )?;
                conn.execute(
                    &format!(
                        "DELETE FROM notifications
                     WHERE NOT ({RETENTION_PROTECTED_PREDICATE})
                       AND id NOT IN (
                         SELECT id FROM notifications
                         WHERE NOT ({RETENTION_PROTECTED_PREDICATE})
                         ORDER BY created_at DESC, id DESC LIMIT ?1
                       )"
                    ),
                    [max_rows],
                )?;
                Ok(())
            })
            .await
    }
}
