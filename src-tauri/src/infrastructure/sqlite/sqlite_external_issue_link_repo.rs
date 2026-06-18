use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

mod rows;

use self::rows::{
    completed_at_for, link_select_sql, now_text, row_to_link, row_to_sync_record, sync_select_sql,
};
use super::DbConnection;
use crate::domain::integrations::{
    ExternalIssueLink, ExternalIssueLinkRepository, ExternalIssueLinkUpsert,
    ExternalIssueLocalObject, ExternalIssueSyncRecord, ExternalIssueSyncRecordUpsert,
    ExternalIssueSyncStatus,
};
use crate::error::AppResult;

pub struct SqliteExternalIssueLinkRepository {
    db: DbConnection,
}

impl SqliteExternalIssueLinkRepository {
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

#[cfg(test)]
#[path = "sqlite_external_issue_link_repo_tests.rs"]
mod tests;

#[async_trait]
impl ExternalIssueLinkRepository for SqliteExternalIssueLinkRepository {
    async fn upsert_link(&self, input: ExternalIssueLinkUpsert) -> AppResult<ExternalIssueLink> {
        self.db
            .run_transaction(move |conn| {
                let existing_id = conn
                    .query_row(
                        "SELECT id FROM external_issue_links
                         WHERE idempotency_key = ?1
                            OR (
                                provider = ?2
                                AND external_kind = ?3
                                AND external_id = ?4
                                AND local_object_kind = ?5
                                AND local_object_id = ?6
                            )
                         LIMIT 1",
                        params![
                            input.idempotency_key,
                            input.provider,
                            input.external_kind,
                            input.external_id,
                            input.local_object.kind.as_str(),
                            input.local_object.id,
                        ],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                let now = now_text();
                let id = existing_id.unwrap_or_else(|| Uuid::new_v4().to_string());
                if conn.execute(
                    "UPDATE external_issue_links
                     SET provider = ?2,
                         external_kind = ?3,
                         external_id = ?4,
                         external_key = ?5,
                         external_url = ?6,
                         local_object_kind = ?7,
                         local_object_id = ?8,
                         local_project_id = ?9,
                         local_sha = ?10,
                         local_state = ?11,
                         idempotency_key = ?12,
                         metadata_json = ?13,
                         updated_at = ?14
                     WHERE id = ?1",
                    params![
                        id,
                        input.provider,
                        input.external_kind,
                        input.external_id,
                        input.external_key,
                        input.external_url,
                        input.local_object.kind.as_str(),
                        input.local_object.id,
                        input.local_project_id,
                        input.local_sha,
                        input.local_state,
                        input.idempotency_key,
                        input.metadata_json,
                        now,
                    ],
                )? == 0
                {
                    conn.execute(
                        "INSERT INTO external_issue_links (
                            id, provider, external_kind, external_id, external_key, external_url,
                            local_object_kind, local_object_id, local_project_id, local_sha,
                            local_state, idempotency_key, metadata_json, created_at, updated_at
                        ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14
                        )",
                        params![
                            id,
                            input.provider,
                            input.external_kind,
                            input.external_id,
                            input.external_key,
                            input.external_url,
                            input.local_object.kind.as_str(),
                            input.local_object.id,
                            input.local_project_id,
                            input.local_sha,
                            input.local_state,
                            input.idempotency_key,
                            input.metadata_json,
                            now,
                        ],
                    )?;
                }

                conn.query_row(
                    &format!("{} WHERE id = ?1", link_select_sql()),
                    [id.as_str()],
                    |row| {
                        row_to_link(row).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    },
                )
                .map_err(Into::into)
            })
            .await
    }

    async fn get_link(&self, id: &str) -> AppResult<Option<ExternalIssueLink>> {
        let id = id.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!("{} WHERE id = ?1", link_select_sql()),
                    [id.as_str()],
                    |row| {
                        row_to_link(row).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    },
                )
            })
            .await
    }

    async fn find_link_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> AppResult<Option<ExternalIssueLink>> {
        let idempotency_key = idempotency_key.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!("{} WHERE idempotency_key = ?1", link_select_sql()),
                    [idempotency_key.as_str()],
                    |row| {
                        row_to_link(row).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    },
                )
            })
            .await
    }

    async fn find_link_by_external_identity(
        &self,
        provider: &str,
        external_kind: &str,
        external_id: &str,
    ) -> AppResult<Option<ExternalIssueLink>> {
        let provider = provider.to_string();
        let external_kind = external_kind.to_string();
        let external_id = external_id.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!(
                        "{} WHERE provider = ?1 AND external_kind = ?2 AND external_id = ?3
                         ORDER BY updated_at DESC LIMIT 1",
                        link_select_sql()
                    ),
                    params![provider, external_kind, external_id],
                    |row| {
                        row_to_link(row).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    },
                )
            })
            .await
    }

    async fn list_links_for_local(
        &self,
        local_object: &ExternalIssueLocalObject,
    ) -> AppResult<Vec<ExternalIssueLink>> {
        let local_kind = local_object.kind.as_str().to_string();
        let local_id = local_object.id.clone();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(&format!(
                    "{} WHERE local_object_kind = ?1 AND local_object_id = ?2
                     ORDER BY updated_at DESC",
                    link_select_sql()
                ))?;
                let links = stmt
                    .query_map(params![local_kind, local_id], |row| {
                        row_to_link(row).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(links)
            })
            .await
    }

    async fn upsert_sync_record(
        &self,
        input: ExternalIssueSyncRecordUpsert,
    ) -> AppResult<ExternalIssueSyncRecord> {
        self.db
            .run_transaction(move |conn| {
                let existing_id = conn
                    .query_row(
                        "SELECT id FROM external_issue_sync_records WHERE idempotency_key = ?1",
                        [input.idempotency_key.as_str()],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                let now = now_text();
                let completed_at = completed_at_for(input.status);
                let id = existing_id.unwrap_or_else(|| Uuid::new_v4().to_string());
                if conn.execute(
                    "UPDATE external_issue_sync_records
                     SET link_id = ?2,
                         sync_kind = ?3,
                         idempotency_key = ?4,
                         local_sha = ?5,
                         local_state = ?6,
                         external_version = ?7,
                         status = ?8,
                         last_attempt_at = ?9,
                         completed_at = ?10,
                         error_message = ?11,
                         metadata_json = ?12,
                         updated_at = ?13
                     WHERE id = ?1",
                    params![
                        id,
                        input.link_id,
                        input.sync_kind,
                        input.idempotency_key,
                        input.local_sha,
                        input.local_state,
                        input.external_version,
                        input.status.as_str(),
                        now,
                        completed_at,
                        input.error_message,
                        input.metadata_json,
                        now,
                    ],
                )? == 0
                {
                    conn.execute(
                        "INSERT INTO external_issue_sync_records (
                            id, link_id, sync_kind, idempotency_key, local_sha, local_state,
                            external_version, status, last_attempt_at, completed_at, error_message,
                            metadata_json, created_at, updated_at
                        ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13
                        )",
                        params![
                            id,
                            input.link_id,
                            input.sync_kind,
                            input.idempotency_key,
                            input.local_sha,
                            input.local_state,
                            input.external_version,
                            input.status.as_str(),
                            now,
                            completed_at,
                            input.error_message,
                            input.metadata_json,
                            now,
                        ],
                    )?;
                }

                conn.query_row(
                    &format!("{} WHERE id = ?1", sync_select_sql()),
                    [id.as_str()],
                    |row| {
                        row_to_sync_record(row).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    },
                )
                .map_err(Into::into)
            })
            .await
    }

    async fn find_sync_record_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> AppResult<Option<ExternalIssueSyncRecord>> {
        let idempotency_key = idempotency_key.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!("{} WHERE idempotency_key = ?1", sync_select_sql()),
                    [idempotency_key.as_str()],
                    |row| {
                        row_to_sync_record(row).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    },
                )
            })
            .await
    }

    async fn list_sync_records_for_link(
        &self,
        link_id: &str,
    ) -> AppResult<Vec<ExternalIssueSyncRecord>> {
        let link_id = link_id.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(&format!(
                    "{} WHERE link_id = ?1 ORDER BY updated_at DESC",
                    sync_select_sql()
                ))?;
                let records = stmt
                    .query_map([link_id.as_str()], |row| {
                        row_to_sync_record(row).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(records)
            })
            .await
    }

    async fn update_sync_status(
        &self,
        sync_record_id: &str,
        status: ExternalIssueSyncStatus,
        external_version: Option<&str>,
        error_message: Option<&str>,
    ) -> AppResult<Option<ExternalIssueSyncRecord>> {
        let sync_record_id = sync_record_id.to_string();
        let external_version = external_version.map(str::to_string);
        let error_message = error_message.map(str::to_string);
        self.db
            .run(move |conn| {
                let now = now_text();
                let completed_at = completed_at_for(status);
                let rows = conn.execute(
                    "UPDATE external_issue_sync_records
                     SET status = ?2,
                         external_version = ?3,
                         error_message = ?4,
                         completed_at = ?5,
                         updated_at = ?6
                     WHERE id = ?1",
                    params![
                        sync_record_id,
                        status.as_str(),
                        external_version,
                        error_message,
                        completed_at,
                        now,
                    ],
                )?;
                if rows == 0 {
                    return Ok(None);
                }

                conn.query_row(
                    &format!("{} WHERE id = ?1", sync_select_sql()),
                    [sync_record_id.as_str()],
                    |row| {
                        row_to_sync_record(row).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    },
                )
                .optional()
                .map_err(Into::into)
            })
            .await
    }
}
