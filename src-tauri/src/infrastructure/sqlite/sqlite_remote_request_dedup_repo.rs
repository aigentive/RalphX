//! SQLite implementation of the remote request-dedup and attachment repositories (§4.3, C-16).
//!
//! Every method goes through [`DbConnection::run`] (rule 16); direct connection locking is
//! forbidden and is asserted against by a source self-scan in the sibling tests.
//!
//! `lookup` maps rusqlite's `QueryReturnedNoRows` to `Absent` and EVERY other rusqlite error to
//! `AppError::Database`. That split is the whole point of the tri-state: only a genuinely
//! successful "no row" read may permit execution.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::entities::{
    RemoteAttachment, RemoteDedupOutcomeKind, RemoteDeviceId, RemoteRequestDedupRecord,
};
use crate::domain::repositories::{
    RemoteAttachmentRepository, RemoteRequestDedupLookup, RemoteRequestDedupRepository,
};
use crate::error::{AppError, AppResult};

const DEDUP_COLUMNS: &str =
    "device_id, request_id, args_hash, outcome, response, created_at, expires_at";
const ATTACHMENT_COLUMNS: &str = "id, device_id, display_name, mime, size, created_at";

/// SQLite-backed store for remote request dedup records and attachment metadata.
pub struct SqliteRemoteRequestDedupRepository {
    db: DbConnection,
}

impl SqliteRemoteRequestDedupRepository {
    pub fn from_db(db: DbConnection) -> Self {
        Self { db }
    }

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

/// Raw column tuple; the `outcome` discriminant is parsed OUTSIDE the rusqlite closure so an
/// unrecognised value surfaces as `AppError::Database` rather than a swallowed mapping failure.
type DedupRow = (String, String, String, String, String, String, String);

fn dedup_row(row: &rusqlite::Row<'_>) -> Result<DedupRow, rusqlite::Error> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

fn dedup_record(raw: DedupRow) -> AppResult<RemoteRequestDedupRecord> {
    let (device_id, request_id, args_hash, outcome, response, created_at, expires_at) = raw;
    let outcome = RemoteDedupOutcomeKind::from_column(&outcome).ok_or_else(|| {
        AppError::Database(format!(
            "unrecognised remote dedup outcome column: {outcome}"
        ))
    })?;
    Ok(RemoteRequestDedupRecord {
        device_id: RemoteDeviceId(device_id),
        request_id,
        args_hash,
        outcome,
        response,
        created_at,
        expires_at,
    })
}

fn attachment_row(row: &rusqlite::Row<'_>) -> Result<RemoteAttachment, rusqlite::Error> {
    Ok(RemoteAttachment {
        id: row.get(0)?,
        device_id: RemoteDeviceId(row.get(1)?),
        display_name: row.get(2)?,
        mime: row.get(3)?,
        size: row.get(4)?,
        created_at: row.get(5)?,
    })
}

#[async_trait]
impl RemoteRequestDedupRepository for SqliteRemoteRequestDedupRepository {
    async fn lookup(
        &self,
        device_id: &RemoteDeviceId,
        request_id: &str,
        now: &str,
    ) -> AppResult<RemoteRequestDedupLookup> {
        let device = device_id.0.clone();
        let request = request_id.to_string();
        let now = now.to_string();
        let raw = self
            .db
            .run(move |conn| {
                let row = conn
                    .query_row(
                        &format!(
                            "SELECT {DEDUP_COLUMNS} FROM remote_request_dedup
                             WHERE device_id = ?1 AND request_id = ?2"
                        ),
                        rusqlite::params![device, request],
                        dedup_row,
                    )
                    .optional()?;
                Ok(row)
            })
            .await?;

        let Some(raw) = raw else {
            return Ok(RemoteRequestDedupLookup::Absent);
        };
        let record = dedup_record(raw)?;
        // RFC3339 UTC timestamps with a fixed shape compare correctly as strings, matching the
        // rest of the remote-access family.
        if record.expires_at.as_str() <= now.as_str() {
            return Ok(RemoteRequestDedupLookup::Expired);
        }
        Ok(RemoteRequestDedupLookup::Fresh(record))
    }

    async fn record(&self, record: RemoteRequestDedupRecord) -> AppResult<()> {
        self.db
            .run(move |conn| {
                conn.execute(
                    &format!(
                        "INSERT INTO remote_request_dedup ({DEDUP_COLUMNS})
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                         ON CONFLICT(device_id, request_id) DO UPDATE SET
                             args_hash = excluded.args_hash,
                             outcome = excluded.outcome,
                             response = excluded.response,
                             created_at = excluded.created_at,
                             expires_at = excluded.expires_at"
                    ),
                    rusqlite::params![
                        record.device_id.0,
                        record.request_id,
                        record.args_hash,
                        record.outcome.as_column(),
                        record.response,
                        record.created_at,
                        record.expires_at,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn purge_expired(&self, now: &str) -> AppResult<usize> {
        let now = now.to_string();
        self.db
            .run(move |conn| {
                let removed = conn.execute(
                    "DELETE FROM remote_request_dedup WHERE expires_at <= ?1",
                    rusqlite::params![now],
                )?;
                Ok(removed)
            })
            .await
    }
}

#[async_trait]
impl RemoteAttachmentRepository for SqliteRemoteRequestDedupRepository {
    async fn record(&self, attachment: RemoteAttachment) -> AppResult<()> {
        self.db
            .run(move |conn| {
                conn.execute(
                    &format!(
                        "INSERT INTO remote_attachments ({ATTACHMENT_COLUMNS})
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
                    ),
                    rusqlite::params![
                        attachment.id,
                        attachment.device_id.0,
                        attachment.display_name,
                        attachment.mime,
                        attachment.size,
                        attachment.created_at,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn get_for_device(
        &self,
        device_id: &RemoteDeviceId,
        id: &str,
    ) -> AppResult<Option<RemoteAttachment>> {
        let device = device_id.0.clone();
        let id = id.to_string();
        self.db
            .run(move |conn| {
                let row = conn
                    .query_row(
                        &format!(
                            "SELECT {ATTACHMENT_COLUMNS} FROM remote_attachments
                             WHERE id = ?1 AND device_id = ?2"
                        ),
                        rusqlite::params![id, device],
                        attachment_row,
                    )
                    .optional()?;
                Ok(row)
            })
            .await
    }

    async fn device_usage_bytes(&self, device_id: &RemoteDeviceId) -> AppResult<i64> {
        let device = device_id.0.clone();
        self.db
            .run(move |conn| {
                // COALESCE keeps the empty-device case at 0 instead of a NULL mapping error.
                let total: i64 = conn.query_row(
                    "SELECT COALESCE(SUM(size), 0) FROM remote_attachments WHERE device_id = ?1",
                    rusqlite::params![device],
                    |row| row.get(0),
                )?;
                Ok(total)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_remote_request_dedup_repo_tests.rs"]
mod tests;
