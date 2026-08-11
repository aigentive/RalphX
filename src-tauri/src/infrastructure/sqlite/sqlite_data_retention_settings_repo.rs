use std::sync::Arc;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::entities::data_retention::{
    DataRetentionDefaults, DataRetentionPolicyUpdate, DataRetentionRunStatus, DataRetentionSettings,
};
use crate::error::{AppError, AppResult};

/// Single-row policy store for chat payload retention.
///
/// Seeding refreshes only the time-window policy and only while the row is still
/// pristine. `payload_size_budget_bytes` / `size_budget_confirmed_at` are excluded
/// from every seeding path: if config could seed a budget, shipping a new default
/// would start deleting user data on installs that never opted in.
pub struct SqliteDataRetentionSettingsRepository {
    db: DbConnection,
}

impl SqliteDataRetentionSettingsRepository {
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

    pub fn from_db(db: DbConnection) -> Self {
        Self { db }
    }

    /// Reads the policy row, seeding or refreshing it from config defaults while pristine.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when the row cannot be read or written.
    pub async fn get_or_seed(
        &self,
        defaults: DataRetentionDefaults,
    ) -> AppResult<DataRetentionSettings> {
        // Config/env values never reach the row unclamped: a seeded `batch_rows = 0`
        // would make every DELETE a silent no-op while the cycle reported success.
        let defaults = defaults.clamped();
        self.db
            .run_transaction(move |conn| {
                let existing = read_row(conn)?;
                let now = Utc::now();
                match existing {
                    Some(settings) if !settings.seeded_pristine => Ok(settings),
                    Some(_) => {
                        conn.execute(
                            "UPDATE data_retention_settings
                             SET payload_retention_enabled = ?1,
                                 payload_retention_days = ?2,
                                 payload_retention_archived_days = ?3,
                                 payload_retention_batch_rows = ?4,
                                 updated_at = ?5
                             WHERE id = 1",
                            params![
                                i64::from(defaults.enabled),
                                defaults.days as i64,
                                defaults.archived_days as i64,
                                defaults.batch_rows as i64,
                                now.to_rfc3339(),
                            ],
                        )?;
                        read_row(conn)?.ok_or_else(|| {
                            AppError::Database(
                                "data retention settings row vanished after refresh".to_string(),
                            )
                        })
                    }
                    None => {
                        conn.execute(
                            "INSERT INTO data_retention_settings (
                                id, payload_retention_enabled, payload_retention_days,
                                payload_retention_archived_days, payload_size_budget_bytes,
                                size_budget_confirmed_at, payload_retention_batch_rows,
                                seeded_pristine, size_budget_advised, updated_at
                             ) VALUES (1, ?1, ?2, ?3, NULL, NULL, ?4, 1, 0, ?5)",
                            params![
                                i64::from(defaults.enabled),
                                defaults.days as i64,
                                defaults.archived_days as i64,
                                defaults.batch_rows as i64,
                                now.to_rfc3339(),
                            ],
                        )?;
                        read_row(conn)?.ok_or_else(|| {
                            AppError::Database(
                                "data retention settings row missing after seed".to_string(),
                            )
                        })
                    }
                }
            })
            .await
    }

    /// Persists a user-authored policy change and clears the pristine flag.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Validation` when bounds or the size-budget consent invariant
    /// are violated, and `AppError::Database` on write failure.
    pub async fn update(
        &self,
        update: DataRetentionPolicyUpdate,
    ) -> AppResult<DataRetentionSettings> {
        update.validate()?;
        self.db
            .run_transaction(move |conn| {
                let now = Utc::now();
                conn.execute(
                    "INSERT INTO data_retention_settings (
                        id, payload_retention_enabled, payload_retention_days,
                        payload_retention_archived_days, payload_size_budget_bytes,
                        size_budget_confirmed_at, payload_retention_batch_rows,
                        seeded_pristine, size_budget_advised, updated_at
                     ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, 0, 0, ?7)
                     ON CONFLICT(id) DO UPDATE SET
                        payload_retention_enabled = excluded.payload_retention_enabled,
                        payload_retention_days = excluded.payload_retention_days,
                        payload_retention_archived_days = excluded.payload_retention_archived_days,
                        payload_size_budget_bytes = excluded.payload_size_budget_bytes,
                        size_budget_confirmed_at = excluded.size_budget_confirmed_at,
                        payload_retention_batch_rows = excluded.payload_retention_batch_rows,
                        seeded_pristine = 0,
                        updated_at = excluded.updated_at",
                    params![
                        i64::from(update.enabled),
                        update.days as i64,
                        update.archived_days as i64,
                        update.size_budget_bytes.map(|bytes| bytes as i64),
                        update
                            .size_budget_confirmed_at
                            .map(|confirmed| confirmed.to_rfc3339()),
                        update.batch_rows as i64,
                        now.to_rfc3339(),
                    ],
                )?;
                read_row(conn)?.ok_or_else(|| {
                    AppError::Database("data retention settings row missing after update".into())
                })
            })
            .await
    }

    /// Records the measurements of a completed retention cycle.
    ///
    /// The advisory flag is persisted here so Settings can render it on open without
    /// running a cycle. Every cycle rewrites it, so it clears itself.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when the status columns cannot be written.
    pub async fn record_run_status(&self, status: DataRetentionRunStatus) -> AppResult<()> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE data_retention_settings
                     SET last_run_at = ?1,
                         last_run_pruned_rows = ?2,
                         last_run_payload_bytes = ?3,
                         last_run_payload_rows = ?4,
                         size_budget_advised = ?5
                     WHERE id = 1",
                    params![
                        status.ran_at.to_rfc3339(),
                        status.pruned_rows as i64,
                        status.payload_bytes.map(|bytes| bytes as i64),
                        status.payload_rows.map(|rows| rows as i64),
                        i64::from(status.size_budget_advised),
                    ],
                )?;
                Ok(())
            })
            .await
    }
}

fn read_row(conn: &Connection) -> AppResult<Option<DataRetentionSettings>> {
    conn.query_row(
        "SELECT payload_retention_enabled, payload_retention_days,
                payload_retention_archived_days, payload_size_budget_bytes,
                size_budget_confirmed_at, payload_retention_batch_rows,
                seeded_pristine, size_budget_advised, last_run_at,
                last_run_pruned_rows, last_run_payload_bytes, last_run_payload_rows,
                updated_at
         FROM data_retention_settings WHERE id = 1",
        [],
        map_row,
    )
    .optional()
    .map_err(|error| AppError::Database(error.to_string()))
}

fn map_row(row: &Row<'_>) -> rusqlite::Result<DataRetentionSettings> {
    Ok(DataRetentionSettings {
        enabled: row.get::<_, i64>(0)? != 0,
        days: row.get::<_, i64>(1)?.max(0) as u64,
        archived_days: row.get::<_, i64>(2)?.max(0) as u64,
        size_budget_bytes: row
            .get::<_, Option<i64>>(3)?
            .map(|bytes| bytes.max(0) as u64),
        size_budget_confirmed_at: parse_optional_timestamp(row.get::<_, Option<String>>(4)?),
        batch_rows: row.get::<_, i64>(5)?.max(0) as u64,
        seeded_pristine: row.get::<_, i64>(6)? != 0,
        size_budget_advised: row.get::<_, i64>(7)? != 0,
        last_run_at: parse_optional_timestamp(row.get::<_, Option<String>>(8)?),
        last_run_pruned_rows: row.get::<_, Option<i64>>(9)?.map(|rows| rows.max(0) as u64),
        last_run_payload_bytes: row
            .get::<_, Option<i64>>(10)?
            .map(|bytes| bytes.max(0) as u64),
        last_run_payload_rows: row
            .get::<_, Option<i64>>(11)?
            .map(|rows| rows.max(0) as u64),
        updated_at: parse_optional_timestamp(row.get::<_, Option<String>>(12)?)
            .unwrap_or_else(Utc::now),
    })
}

fn parse_optional_timestamp(value: Option<String>) -> Option<DateTime<Utc>> {
    value.and_then(|raw| {
        DateTime::parse_from_rfc3339(&raw)
            .ok()
            .map(|parsed| parsed.with_timezone(&Utc))
    })
}
