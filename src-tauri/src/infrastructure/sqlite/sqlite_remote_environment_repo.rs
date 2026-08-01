// SQLite-based RemoteEnvironmentRepository implementation (client registry, §6.1).
//
// All methods go through DbConnection::run / run_transaction (rule 16 / C-1); the
// pairing upsert is transactional so the environment_id dedup merge can never race
// itself into two rows for one host.

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use super::DbConnection;
use crate::domain::entities::remote_environment::{
    remote_environment_token_secret_ref, RemoteEnvironment, RemoteEnvironmentId,
    RemoteEnvironmentStatus,
};
use crate::domain::repositories::{RemoteEnvironmentRepository, UpsertPairedEnvironment};
use crate::error::{AppError, AppResult};

const SELECT_COLUMNS: &str = "id, environment_id, name, base_url, candidate_urls, \
     token_secret_ref, scopes, protocol_version, status, created_at, last_connected_at";

pub struct SqliteRemoteEnvironmentRepository {
    db: DbConnection,
}

impl SqliteRemoteEnvironmentRepository {
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

fn parse_json_strings(field: &str, raw: &str) -> AppResult<Vec<String>> {
    serde_json::from_str::<Vec<String>>(raw)
        .map_err(|error| AppError::Database(format!("invalid {field} JSON: {error}")))
}

fn parse_scopes(raw: &str) -> AppResult<Vec<ralphx_remote_protocol::Scope>> {
    serde_json::from_str::<Vec<ralphx_remote_protocol::Scope>>(raw)
        .map_err(|error| AppError::Database(format!("invalid remote environment scopes: {error}")))
}

fn serialize_json<T: serde::Serialize>(field: &str, value: &T) -> AppResult<String> {
    serde_json::to_string(value)
        .map_err(|error| AppError::Database(format!("failed to serialize {field}: {error}")))
}

type RemoteEnvironmentRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    Option<String>,
);

fn row_to_tuple(row: &rusqlite::Row<'_>) -> Result<RemoteEnvironmentRow, rusqlite::Error> {
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
        row.get(10)?,
    ))
}

fn tuple_to_environment(tuple: RemoteEnvironmentRow) -> AppResult<RemoteEnvironment> {
    let (
        id,
        environment_id,
        name,
        base_url,
        candidate_urls,
        token_secret_ref,
        scopes,
        protocol_version,
        status,
        created_at,
        last_connected_at,
    ) = tuple;
    let status = RemoteEnvironmentStatus::parse(&status).ok_or_else(|| {
        AppError::Database(format!("invalid remote environment status: {status}"))
    })?;
    let protocol_version = u32::try_from(protocol_version).map_err(|_| {
        AppError::Database(format!(
            "invalid remote environment protocol version: {protocol_version}"
        ))
    })?;
    Ok(RemoteEnvironment {
        id: RemoteEnvironmentId::from_string(id),
        environment_id,
        name,
        base_url,
        candidate_urls: parse_json_strings("candidate_urls", &candidate_urls)?,
        token_secret_ref,
        scopes: parse_scopes(&scopes)?,
        protocol_version,
        status,
        created_at,
        last_connected_at,
    })
}

fn read_by_environment_id(
    conn: &Connection,
    environment_id: &str,
) -> AppResult<Option<RemoteEnvironment>> {
    let result = conn.query_row(
        &format!("SELECT {SELECT_COLUMNS} FROM remote_environments WHERE environment_id = ?1"),
        [environment_id],
        row_to_tuple,
    );
    match result {
        Ok(tuple) => Ok(Some(tuple_to_environment(tuple)?)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(AppError::Database(error.to_string())),
    }
}

fn read_by_id(conn: &Connection, id: &str) -> AppResult<Option<RemoteEnvironment>> {
    let result = conn.query_row(
        &format!("SELECT {SELECT_COLUMNS} FROM remote_environments WHERE id = ?1"),
        [id],
        row_to_tuple,
    );
    match result {
        Ok(tuple) => Ok(Some(tuple_to_environment(tuple)?)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(AppError::Database(error.to_string())),
    }
}

#[async_trait]
impl RemoteEnvironmentRepository for SqliteRemoteEnvironmentRepository {
    async fn upsert_paired(&self, params: UpsertPairedEnvironment) -> AppResult<RemoteEnvironment> {
        self.db
            .run_transaction(move |conn| {
                let scopes_json = serialize_json("scopes", &params.scopes)?;
                match read_by_environment_id(conn, &params.environment_id)? {
                    Some(existing) => {
                        // Dedup merge (§6.1): same host through a second URL keeps the
                        // one row (and its Keychain ref) and records the new endpoint.
                        let mut candidate_urls = existing.candidate_urls.clone();
                        if params.url != existing.base_url && !candidate_urls.contains(&params.url)
                        {
                            candidate_urls.push(params.url.clone());
                        }
                        let candidate_urls_json =
                            serialize_json("candidate_urls", &candidate_urls)?;
                        conn.execute(
                            "UPDATE remote_environments
                             SET name = ?2, candidate_urls = ?3, scopes = ?4,
                                 protocol_version = ?5, status = ?6
                             WHERE id = ?1",
                            rusqlite::params![
                                existing.id.as_str(),
                                params.name,
                                candidate_urls_json,
                                scopes_json,
                                i64::from(params.protocol_version),
                                RemoteEnvironmentStatus::PendingAdd.as_str(),
                            ],
                        )
                        .map_err(|error| AppError::Database(error.to_string()))?;
                        read_by_id(conn, existing.id.as_str())?.ok_or_else(|| {
                            AppError::Database(
                                "remote environment row vanished during upsert".to_string(),
                            )
                        })
                    }
                    None => {
                        let id = RemoteEnvironmentId::new();
                        let token_secret_ref = remote_environment_token_secret_ref(&id);
                        conn.execute(
                            "INSERT INTO remote_environments (
                                id, environment_id, name, base_url, candidate_urls,
                                token_secret_ref, scopes, protocol_version, status
                             ) VALUES (?1, ?2, ?3, ?4, '[]', ?5, ?6, ?7, ?8)",
                            rusqlite::params![
                                id.as_str(),
                                params.environment_id,
                                params.name,
                                params.url,
                                token_secret_ref,
                                scopes_json,
                                i64::from(params.protocol_version),
                                RemoteEnvironmentStatus::PendingAdd.as_str(),
                            ],
                        )
                        .map_err(|error| AppError::Database(error.to_string()))?;
                        read_by_id(conn, id.as_str())?.ok_or_else(|| {
                            AppError::Database(
                                "remote environment row vanished during insert".to_string(),
                            )
                        })
                    }
                }
            })
            .await
    }

    async fn get(&self, id: &RemoteEnvironmentId) -> AppResult<Option<RemoteEnvironment>> {
        let id = id.as_str().to_string();
        self.db.run(move |conn| read_by_id(conn, &id)).await
    }

    async fn get_by_environment_id(
        &self,
        environment_id: &str,
    ) -> AppResult<Option<RemoteEnvironment>> {
        let environment_id = environment_id.to_string();
        self.db
            .run(move |conn| read_by_environment_id(conn, &environment_id))
            .await
    }

    async fn list(&self) -> AppResult<Vec<RemoteEnvironment>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {SELECT_COLUMNS} FROM remote_environments ORDER BY created_at ASC, id ASC"
                ))?;
                let tuples = stmt
                    .query_map([], row_to_tuple)?
                    .collect::<Result<Vec<_>, _>>()?;
                tuples.into_iter().map(tuple_to_environment).collect()
            })
            .await
    }

    async fn set_status(
        &self,
        id: &RemoteEnvironmentId,
        status: RemoteEnvironmentStatus,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let updated = conn
                    .execute(
                        "UPDATE remote_environments SET status = ?2 WHERE id = ?1",
                        rusqlite::params![id, status.as_str()],
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                if updated == 0 {
                    return Err(AppError::NotFound(format!(
                        "remote environment not found: {id}"
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &RemoteEnvironmentId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM remote_environments WHERE id = ?1",
                    rusqlite::params![id],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(())
            })
            .await
    }

    async fn touch_last_connected(
        &self,
        id: &RemoteEnvironmentId,
        timestamp: &str,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        let timestamp = timestamp.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE remote_environments SET last_connected_at = ?2 WHERE id = ?1",
                    rusqlite::params![id, timestamp],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_remote_environment_repo_tests.rs"]
mod tests;
