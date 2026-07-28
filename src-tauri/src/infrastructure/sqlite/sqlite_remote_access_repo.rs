//! SQLite implementation of the :3849 remote-access repositories (§4.3).
//!
//! Every method goes through [`DbConnection::run`] / [`DbConnection::run_transaction`]
//! (rule 16) — no `conn.lock().await` anywhere. Pairing-code redemption and ticket
//! consumption run inside `run_transaction` (`BEGIN IMMEDIATE`) so a concurrent second
//! redemption always loses the guarded `consumed_at IS NULL` update (P-7).

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::entities::{
    effective_pairing_scopes, RemoteAuditAction, RemoteAuditEntry, RemoteDevice, RemoteDeviceId,
    RemotePairingCode, RemotePairingCodeId, RemoteScopeSet, RemoteSession, RemoteSessionId,
};
use crate::domain::repositories::{
    RemoteAuditLogRepository, RemoteDeviceLookup, RemoteDeviceRepository,
    RemotePairingCodeRepository, RemotePairingOutcome, RemotePairingRedemption,
    RemoteSessionRepository, RemoteWsTicketOutcome, RemoteWsTicketRepository,
};
use crate::error::{AppError, AppResult};

const DEVICE_COLUMNS: &str =
    "id, name, token_hash, token_prefix, scopes, created_at, last_seen_at, revoked_at";
const PAIRING_CODE_COLUMNS: &str = "id, code_hash, scopes, created_at, expires_at, consumed_at";
const SESSION_COLUMNS: &str = "id, device_id, connected_at, last_active_at, remote_addr, closed_at";

/// SQLite-backed store for remote devices, pairing codes, sessions, tickets, and audit rows.
pub struct SqliteRemoteAccessRepository {
    db: DbConnection,
}

impl SqliteRemoteAccessRepository {
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

/// Scope columns are parsed strictly: a malformed grant is a store error, never an empty set.
fn scopes_from_column(raw: String) -> AppResult<RemoteScopeSet> {
    RemoteScopeSet::from_json(&raw).map_err(|error| AppError::Database(error.to_string()))
}

fn scopes_to_column(scopes: &RemoteScopeSet) -> AppResult<String> {
    scopes
        .to_json()
        .map_err(|error| AppError::Database(error.to_string()))
}

/// Raw column tuple for a device row; scope parsing happens outside the rusqlite closure so a
/// malformed grant surfaces as `AppError::Database`, not a swallowed row-mapping failure.
type DeviceRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

fn device_row(row: &rusqlite::Row<'_>) -> Result<DeviceRow, rusqlite::Error> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

fn device_from_row(raw: DeviceRow) -> AppResult<RemoteDevice> {
    Ok(RemoteDevice {
        id: RemoteDeviceId::from_string(raw.0),
        name: raw.1,
        token_hash: raw.2,
        token_prefix: raw.3,
        scopes: scopes_from_column(raw.4)?,
        created_at: raw.5,
        last_seen_at: raw.6,
        revoked_at: raw.7,
    })
}

fn read_device(conn: &Connection, id: &str) -> AppResult<Option<RemoteDevice>> {
    let raw = conn.query_row(
        &format!("SELECT {DEVICE_COLUMNS} FROM remote_devices WHERE id = ?1"),
        rusqlite::params![id],
        device_row,
    );
    match raw {
        Ok(raw) => Ok(Some(device_from_row(raw)?)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(AppError::Database(error.to_string())),
    }
}

type PairingCodeRow = (String, String, String, String, String, Option<String>);

fn pairing_code_row(row: &rusqlite::Row<'_>) -> Result<PairingCodeRow, rusqlite::Error> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn pairing_code_from_row(raw: PairingCodeRow) -> AppResult<RemotePairingCode> {
    Ok(RemotePairingCode {
        id: RemotePairingCodeId::from_string(raw.0),
        code_hash: raw.1,
        scopes: scopes_from_column(raw.2)?,
        created_at: raw.3,
        expires_at: raw.4,
        consumed_at: raw.5,
    })
}

type SessionRow = (String, String, String, String, String, Option<String>);

fn session_row(row: &rusqlite::Row<'_>) -> Result<SessionRow, rusqlite::Error> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn session_from_row(raw: SessionRow) -> RemoteSession {
    RemoteSession {
        id: RemoteSessionId::from_string(raw.0),
        device_id: RemoteDeviceId::from_string(raw.1),
        connected_at: raw.2,
        last_active_at: raw.3,
        remote_addr: raw.4,
        closed_at: raw.5,
    }
}

#[async_trait]
impl RemoteDeviceRepository for SqliteRemoteAccessRepository {
    async fn lookup_by_token_hash(&self, token_hash: &str) -> AppResult<RemoteDeviceLookup> {
        let token_hash = token_hash.to_string();
        self.db
            .run(move |conn| {
                let raw = conn.query_row(
                    &format!("SELECT {DEVICE_COLUMNS} FROM remote_devices WHERE token_hash = ?1"),
                    rusqlite::params![token_hash],
                    device_row,
                );
                match raw {
                    Ok(raw) => {
                        let device = device_from_row(raw)?;
                        Ok(if device.is_active() {
                            RemoteDeviceLookup::Active(device)
                        } else {
                            RemoteDeviceLookup::Revoked(device)
                        })
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(RemoteDeviceLookup::Unknown),
                    // Deliberately an error, not `Unknown`: a store outage must not be
                    // observable as "this token simply is not paired" (§4.4).
                    Err(error) => Err(AppError::Database(error.to_string())),
                }
            })
            .await
    }

    async fn get(&self, id: &RemoteDeviceId) -> AppResult<Option<RemoteDevice>> {
        let id = id.to_string();
        self.db.run(move |conn| read_device(conn, &id)).await
    }

    async fn list(&self) -> AppResult<Vec<RemoteDevice>> {
        self.db
            .run(move |conn| {
                let mut statement = conn
                    .prepare(&format!(
                        "SELECT {DEVICE_COLUMNS} FROM remote_devices ORDER BY created_at DESC, id DESC"
                    ))
                    .map_err(|error| AppError::Database(error.to_string()))?;
                let rows = statement
                    .query_map([], device_row)
                    .map_err(|error| AppError::Database(error.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| AppError::Database(error.to_string()))?;
                rows.into_iter().map(device_from_row).collect()
            })
            .await
    }

    async fn revoke(&self, id: &RemoteDeviceId, now: &str) -> AppResult<Option<RemoteDevice>> {
        let id = id.to_string();
        let now = now.to_string();
        self.db
            .run_transaction(move |conn| {
                conn.execute(
                    "UPDATE remote_devices SET revoked_at = ?1
                     WHERE id = ?2 AND revoked_at IS NULL",
                    rusqlite::params![now, id],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                read_device(conn, &id)
            })
            .await
    }

    async fn set_scopes(
        &self,
        id: &RemoteDeviceId,
        scopes: &RemoteScopeSet,
    ) -> AppResult<Option<RemoteDevice>> {
        let id = id.to_string();
        let encoded = scopes_to_column(scopes)?;
        self.db
            .run_transaction(move |conn| {
                // `revoked_at IS NULL` keeps a toggle from widening a dead credential.
                conn.execute(
                    "UPDATE remote_devices SET scopes = ?1 WHERE id = ?2 AND revoked_at IS NULL",
                    rusqlite::params![encoded, id],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                read_device(conn, &id)
            })
            .await
    }

    async fn touch_last_seen(&self, id: &RemoteDeviceId, now: &str) -> AppResult<()> {
        let id = id.to_string();
        let now = now.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE remote_devices SET last_seen_at = ?1 WHERE id = ?2",
                    rusqlite::params![now, id],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(())
            })
            .await
    }
}

#[async_trait]
impl RemotePairingCodeRepository for SqliteRemoteAccessRepository {
    async fn create(&self, code: RemotePairingCode) -> AppResult<RemotePairingCode> {
        let encoded = scopes_to_column(&code.scopes)?;
        let stored = code.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO remote_pairing_codes
                        (id, code_hash, scopes, created_at, expires_at, consumed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        stored.id.as_str(),
                        stored.code_hash,
                        encoded,
                        stored.created_at,
                        stored.expires_at,
                        stored.consumed_at,
                    ],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(stored)
            })
            .await
    }

    async fn redeem(&self, redemption: RemotePairingRedemption) -> AppResult<RemotePairingOutcome> {
        self.db
            .run_transaction(move |conn| {
                let raw = conn.query_row(
                    &format!(
                        "SELECT {PAIRING_CODE_COLUMNS} FROM remote_pairing_codes
                         WHERE code_hash = ?1"
                    ),
                    rusqlite::params![redemption.code_hash],
                    pairing_code_row,
                );
                let code = match raw {
                    Ok(raw) => pairing_code_from_row(raw)?,
                    Err(rusqlite::Error::QueryReturnedNoRows) => {
                        return Ok(RemotePairingOutcome::Unknown)
                    }
                    Err(error) => return Err(AppError::Database(error.to_string())),
                };
                if code.consumed_at.is_some() {
                    return Ok(RemotePairingOutcome::AlreadyConsumed);
                }
                // String comparison is valid here: every timestamp this crate writes is
                // fixed-width RFC3339 UTC, so lexicographic order is chronological order.
                if code.expires_at <= redemption.now {
                    return Ok(RemotePairingOutcome::Expired);
                }

                let scopes = match effective_pairing_scopes(
                    &code.scopes,
                    redemption.requested_scopes.as_ref(),
                ) {
                    Ok(scopes) => scopes,
                    Err(crate::domain::entities::RemoteScopeError::NotGranted(scope)) => {
                        return Ok(RemotePairingOutcome::ScopeNotGranted(scope))
                    }
                    Err(error) => return Err(AppError::Validation(error.to_string())),
                };

                // The guard is what makes redemption single-use under concurrency: the
                // loser of two `BEGIN IMMEDIATE` transactions updates zero rows (P-7).
                let consumed = conn
                    .execute(
                        "UPDATE remote_pairing_codes SET consumed_at = ?1
                         WHERE id = ?2 AND consumed_at IS NULL",
                        rusqlite::params![redemption.now, code.id.as_str()],
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                if consumed != 1 {
                    return Ok(RemotePairingOutcome::AlreadyConsumed);
                }

                let encoded = scopes_to_column(&scopes)?;
                conn.execute(
                    "INSERT INTO remote_devices
                        (id, name, token_hash, token_prefix, scopes, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        redemption.device_id.as_str(),
                        redemption.device_name,
                        redemption.token_hash,
                        redemption.token_prefix,
                        encoded,
                        redemption.now,
                    ],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;

                // Same transaction as the mint: a pairing that commits always leaves a trail,
                // and an audit failure rolls the device back instead of stranding an active
                // credential whose token was never delivered.
                conn.execute(
                    "INSERT INTO remote_audit_log (device_id, action, detail, created_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        redemption.device_id.as_str(),
                        RemoteAuditAction::PairingSucceeded.as_db_value(),
                        redemption.audit_detail,
                        redemption.now,
                    ],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;

                let device =
                    read_device(conn, redemption.device_id.as_str())?.ok_or_else(|| {
                        AppError::Database(
                            "paired device row disappeared inside its own transaction".to_string(),
                        )
                    })?;
                Ok(RemotePairingOutcome::Paired(device))
            })
            .await
    }

    async fn list_outstanding(&self, now: &str) -> AppResult<Vec<RemotePairingCode>> {
        let now = now.to_string();
        self.db
            .run(move |conn| {
                let mut statement = conn
                    .prepare(&format!(
                        "SELECT {PAIRING_CODE_COLUMNS} FROM remote_pairing_codes
                         WHERE consumed_at IS NULL AND expires_at > ?1
                         ORDER BY created_at DESC, id DESC"
                    ))
                    .map_err(|error| AppError::Database(error.to_string()))?;
                let rows = statement
                    .query_map(rusqlite::params![now], pairing_code_row)
                    .map_err(|error| AppError::Database(error.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| AppError::Database(error.to_string()))?;
                rows.into_iter().map(pairing_code_from_row).collect()
            })
            .await
    }

    async fn cancel(&self, id: &RemotePairingCodeId, now: &str) -> AppResult<bool> {
        let id = id.to_string();
        let now = now.to_string();
        self.db
            .run(move |conn| {
                let affected = conn
                    .execute(
                        "UPDATE remote_pairing_codes SET consumed_at = ?1
                         WHERE id = ?2 AND consumed_at IS NULL",
                        rusqlite::params![now, id],
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(affected == 1)
            })
            .await
    }
}

#[async_trait]
impl RemoteSessionRepository for SqliteRemoteAccessRepository {
    async fn open(&self, session: RemoteSession) -> AppResult<RemoteSession> {
        let stored = session.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO remote_sessions
                        (id, device_id, connected_at, last_active_at, remote_addr, closed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        stored.id.as_str(),
                        stored.device_id.as_str(),
                        stored.connected_at,
                        stored.last_active_at,
                        stored.remote_addr,
                        stored.closed_at,
                    ],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(stored)
            })
            .await
    }

    async fn touch(&self, id: &RemoteSessionId, now: &str) -> AppResult<()> {
        let id = id.to_string();
        let now = now.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE remote_sessions SET last_active_at = ?1 WHERE id = ?2",
                    rusqlite::params![now, id],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(())
            })
            .await
    }

    async fn close(&self, id: &RemoteSessionId, now: &str) -> AppResult<()> {
        let id = id.to_string();
        let now = now.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE remote_sessions SET closed_at = ?1
                     WHERE id = ?2 AND closed_at IS NULL",
                    rusqlite::params![now, id],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(())
            })
            .await
    }

    async fn close_all_for_device(
        &self,
        device_id: &RemoteDeviceId,
        now: &str,
    ) -> AppResult<usize> {
        let device_id = device_id.to_string();
        let now = now.to_string();
        self.db
            .run(move |conn| {
                let affected = conn
                    .execute(
                        "UPDATE remote_sessions SET closed_at = ?1
                         WHERE device_id = ?2 AND closed_at IS NULL",
                        rusqlite::params![now, device_id],
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(affected)
            })
            .await
    }

    async fn close_all(&self, now: &str) -> AppResult<usize> {
        let now = now.to_string();
        self.db
            .run(move |conn| {
                let affected = conn
                    .execute(
                        "UPDATE remote_sessions SET closed_at = ?1 WHERE closed_at IS NULL",
                        rusqlite::params![now],
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(affected)
            })
            .await
    }

    async fn list_open(&self) -> AppResult<Vec<RemoteSession>> {
        self.db
            .run(move |conn| {
                let mut statement = conn
                    .prepare(&format!(
                        "SELECT {SESSION_COLUMNS} FROM remote_sessions
                         WHERE closed_at IS NULL
                         ORDER BY connected_at DESC, id DESC"
                    ))
                    .map_err(|error| AppError::Database(error.to_string()))?;
                let rows = statement
                    .query_map([], session_row)
                    .map_err(|error| AppError::Database(error.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(rows.into_iter().map(session_from_row).collect())
            })
            .await
    }
}

#[async_trait]
impl RemoteWsTicketRepository for SqliteRemoteAccessRepository {
    async fn issue(
        &self,
        ticket_hash: &str,
        device_id: &RemoteDeviceId,
        expires_at: &str,
    ) -> AppResult<()> {
        let ticket_hash = ticket_hash.to_string();
        let device_id = device_id.to_string();
        let expires_at = expires_at.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO remote_ws_tickets (ticket_hash, device_id, expires_at)
                     VALUES (?1, ?2, ?3)",
                    rusqlite::params![ticket_hash, device_id, expires_at],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(())
            })
            .await
    }

    async fn consume(&self, ticket_hash: &str, now: &str) -> AppResult<RemoteWsTicketOutcome> {
        let ticket_hash = ticket_hash.to_string();
        let now = now.to_string();
        self.db
            .run_transaction(move |conn| {
                let raw = conn.query_row(
                    "SELECT device_id, expires_at, consumed_at FROM remote_ws_tickets
                     WHERE ticket_hash = ?1",
                    rusqlite::params![ticket_hash],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                );
                let (device_id, expires_at, consumed_at) = match raw {
                    Ok(row) => row,
                    Err(rusqlite::Error::QueryReturnedNoRows) => {
                        return Ok(RemoteWsTicketOutcome::Unknown)
                    }
                    Err(error) => return Err(AppError::Database(error.to_string())),
                };
                if consumed_at.is_some() {
                    return Ok(RemoteWsTicketOutcome::AlreadyConsumed);
                }
                if expires_at <= now {
                    return Ok(RemoteWsTicketOutcome::Expired);
                }
                let consumed = conn
                    .execute(
                        "UPDATE remote_ws_tickets SET consumed_at = ?1
                         WHERE ticket_hash = ?2 AND consumed_at IS NULL",
                        rusqlite::params![now, ticket_hash],
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                if consumed != 1 {
                    return Ok(RemoteWsTicketOutcome::AlreadyConsumed);
                }
                Ok(RemoteWsTicketOutcome::Consumed(
                    RemoteDeviceId::from_string(device_id),
                ))
            })
            .await
    }

    async fn consume_all_for_device(
        &self,
        device_id: &RemoteDeviceId,
        now: &str,
    ) -> AppResult<usize> {
        let device_id = device_id.to_string();
        let now = now.to_string();
        self.db
            .run(move |conn| {
                let affected = conn
                    .execute(
                        "UPDATE remote_ws_tickets SET consumed_at = ?1
                         WHERE device_id = ?2 AND consumed_at IS NULL",
                        rusqlite::params![now, device_id],
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(affected)
            })
            .await
    }
}

#[async_trait]
impl RemoteAuditLogRepository for SqliteRemoteAccessRepository {
    async fn record(
        &self,
        device_id: Option<&RemoteDeviceId>,
        action: RemoteAuditAction,
        detail: Option<&str>,
        now: &str,
    ) -> AppResult<()> {
        let device_id = device_id.map(|id| id.to_string());
        let detail = detail.map(|detail| detail.to_string());
        let now = now.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO remote_audit_log (device_id, action, detail, created_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![device_id, action.as_db_value(), detail, now],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(())
            })
            .await
    }

    async fn list_recent(&self, limit: Option<i64>) -> AppResult<Vec<RemoteAuditEntry>> {
        let limit = limit.unwrap_or(100).clamp(1, 1000);
        self.db
            .run(move |conn| {
                let mut statement = conn
                    .prepare(
                        "SELECT id, device_id, action, detail, created_at FROM remote_audit_log
                         ORDER BY id DESC LIMIT ?1",
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                let rows = statement
                    .query_map(rusqlite::params![limit], |row| {
                        Ok(RemoteAuditEntry {
                            id: row.get(0)?,
                            device_id: row
                                .get::<_, Option<String>>(1)?
                                .map(RemoteDeviceId::from_string),
                            action: row.get(2)?,
                            detail: row.get(3)?,
                            created_at: row.get(4)?,
                        })
                    })
                    .map_err(|error| AppError::Database(error.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(rows)
            })
            .await
    }

    async fn prune_before(&self, cutoff: &str) -> AppResult<usize> {
        let cutoff = cutoff.to_string();
        self.db
            .run(move |conn| {
                // Lexicographic comparison is chronological here: every timestamp this crate
                // writes is fixed-width RFC3339 UTC.
                let affected = conn
                    .execute(
                        "DELETE FROM remote_audit_log WHERE created_at < ?1",
                        rusqlite::params![cutoff],
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(affected)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_remote_access_repo_tests.rs"]
mod tests;
