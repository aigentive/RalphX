//! SQLite implementation of the durable remote event log (§3.4).
//!
//! Two access rules this module exists to enforce, both load-bearing:
//!
//! 1. **Commits go through [`DbConnection::run_transaction`]** (`BEGIN IMMEDIATE`) and write the
//!    rows *and* the seq high-water in the same transaction, so the counter can never disagree
//!    with the log (§3.4 #2, C-14).
//! 2. **Catch-up drains go through [`DbConnection::run`]** — a plain autocommit read. Using
//!    `run_transaction` would take a write-intent lock for the length of a 50k-row replay and
//!    serialize against the sequencer's own commits, stalling live delivery for every client
//!    (N-M2). The retention lease, not a transaction, is what keeps the rows alive under the
//!    drain.

use async_trait::async_trait;
use rusqlite::Connection;

use super::DbConnection;
use crate::error::{AppError, AppResult};

/// Settings row that carries the durable seq high-water.
const SETTINGS_ROW_ID: i64 = 1;

/// Rows deleted per prune transaction. Retention is a background reclaim; it must never hold one
/// `BEGIN IMMEDIATE` for an unbounded delete, or a single long transaction parks every other
/// caller (auth reads, audit writes) for the full `busy_timeout` window on a large log. Each chunk
/// is its own transaction, releasing the connection between chunks so contended callers interleave.
const PRUNE_CHUNK_SIZE: u64 = 2_000;

/// A durable event row exactly as the sequencer commits and replays it.
///
/// `payload` is the **raw** JSON text Tauri delivered. It is never re-serialized on the way in
/// or out: `remote_event_log.payload` is TEXT, the capture handler hands over an unparsed
/// string (`remote_server/capture.rs`), and replay hands the same bytes back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEventRow {
    pub seq: u64,
    pub epoch: String,
    pub name: String,
    pub payload: String,
}

/// Everything the pruner needs to compute a ceiling it can prove is safe.
///
/// `lease_floor` is the only field that can *lower* the ceiling below the retention policy; it
/// is `u64::MAX` when no session holds a lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePruneRequest {
    /// The live in-memory `streamEpoch`. Rows from any other epoch are unreplayable.
    pub current_epoch: String,
    /// Durable high-water at the moment `current_epoch` was minted; every row at or below it
    /// belongs to a prior epoch by construction.
    pub epoch_floor_seq: u64,
    /// Durable high-water now.
    pub max_seq: u64,
    /// `min(live lease cursors)`, or `u64::MAX` when nothing is leased. **The pruner may never
    /// delete a row above this** (R4-H3, corrected inequality).
    pub lease_floor: u64,
    /// Row-count window to retain (50 000).
    pub retain_rows: u64,
    /// Age window to retain, in days (7).
    pub retain_days: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemotePruneOutcome {
    pub deleted: u64,
    /// Every row with `seq <= pruned_floor` is now gone; a subscriber below it must `reset`.
    pub pruned_floor: u64,
}

/// The durable event log seam.
///
/// A trait rather than a concrete type so the sequencer and the WS replay path can be tested
/// against induced commit failures and induced read errors — `reset(read_error)` is only
/// provable if a read *can* fail (P-5(c)).
#[async_trait]
pub trait RemoteEventLogStore: Send + Sync {
    /// The persisted seq high-water. Monotonic across boots, never reused.
    async fn high_water(&self) -> AppResult<u64>;

    /// Commits a pre-allocated batch and the resulting high-water atomically.
    async fn append_batch(&self, rows: &[RemoteEventRow]) -> AppResult<()>;

    /// Plain bounded read of `after_seq < seq <= through_seq` within one epoch, ascending.
    async fn read_range(
        &self,
        epoch: &str,
        after_seq: u64,
        through_seq: u64,
        limit: usize,
    ) -> AppResult<Vec<RemoteEventRow>>;

    /// Lowest surviving seq, used to derive the pruned floor at startup.
    async fn oldest_seq(&self) -> AppResult<Option<u64>>;

    async fn prune(&self, request: RemotePruneRequest) -> AppResult<RemotePruneOutcome>;
}

/// The prune ceiling — the whole retention-safety argument in one function.
///
/// Returns the highest seq the pruner may delete. Two properties are asserted by construction:
///
/// * the result is **never above `lease_floor`**, so no row a live cursor still needs is
///   deletable (R4-H3: a lease at `C` has consumed `<= C` and needs `> C`);
/// * prior-epoch rows (`seq <= epoch_floor_seq`) are eligible even when the retention window
///   would keep them, because no client can ever replay them (§3.4 DDL note) — still clamped by
///   `lease_floor`, so the clamp is the single safety boundary.
pub fn prune_ceiling(retention_ceiling: u64, epoch_floor_seq: u64, lease_floor: u64) -> u64 {
    retention_ceiling.max(epoch_floor_seq).min(lease_floor)
}

/// Highest seq the retention policy alone would drop.
///
/// A row must be outside **both** windows to be eligible — `max(50 000 rows, 7 days)` retains
/// the larger window, so eligibility is the intersection.
pub fn retention_ceiling(max_seq: u64, retain_rows: u64, age_ceiling: u64) -> u64 {
    max_seq.saturating_sub(retain_rows).min(age_ceiling)
}

pub struct SqliteRemoteEventLogRepository {
    db: DbConnection,
}

impl SqliteRemoteEventLogRepository {
    pub fn from_db(db: DbConnection) -> Self {
        Self { db }
    }
}

#[async_trait]
impl RemoteEventLogStore for SqliteRemoteEventLogRepository {
    async fn high_water(&self) -> AppResult<u64> {
        self.db.run(move |conn| read_high_water(conn)).await
    }

    async fn append_batch(&self, rows: &[RemoteEventRow]) -> AppResult<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let rows = rows.to_vec();
        self.db
            .run_transaction(move |conn| {
                let mut statement = conn
                    .prepare(
                        "INSERT INTO remote_event_log (seq, epoch, name, payload)
                         VALUES (?1, ?2, ?3, ?4)",
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                let mut high_water = 0_u64;
                for row in &rows {
                    statement
                        .execute(rusqlite::params![
                            seq_to_sql(row.seq)?,
                            row.epoch,
                            row.name,
                            row.payload,
                        ])
                        .map_err(|error| AppError::Database(error.to_string()))?;
                    high_water = high_water.max(row.seq);
                }
                drop(statement);
                // Same transaction as the rows: a committed seq is always a persisted high-water,
                // and a rolled-back batch leaves neither behind.
                let updated = conn
                    .execute(
                        "UPDATE remote_host_settings
                         SET event_seq_high_water = ?1
                         WHERE id = ?2 AND event_seq_high_water < ?1",
                        rusqlite::params![seq_to_sql(high_water)?, SETTINGS_ROW_ID],
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                // An UPDATE that touched nothing means either the stored high-water is already at
                // or above this batch (benign) or the settings row is ABSENT — and a missing row
                // would let the log rows commit without their high-water. The next boot would then
                // seed `next_seq` below seqs that still exist in the log, every insert would
                // collide on the seq primary key, and the resulting commit-failure→epoch-roll loop
                // would kill the durable stream for that whole boot. Fail loudly at the sink
                // instead of leaving the wedge for a later process to hit (C-14).
                if updated == 0 && read_high_water(conn)? < high_water {
                    return Err(AppError::Database(format!(
                        "remote event high-water {high_water} was not persisted: the remote_host_settings row is missing"
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn read_range(
        &self,
        epoch: &str,
        after_seq: u64,
        through_seq: u64,
        limit: usize,
    ) -> AppResult<Vec<RemoteEventRow>> {
        let epoch = epoch.to_string();
        // Plain read, deliberately NOT run_transaction (N-M2).
        self.db
            .run(move |conn| {
                let mut statement = conn
                    .prepare(
                        "SELECT seq, epoch, name, payload
                         FROM remote_event_log
                         WHERE epoch = ?1 AND seq > ?2 AND seq <= ?3
                         ORDER BY seq
                         LIMIT ?4",
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                let rows = statement
                    .query_map(
                        rusqlite::params![
                            epoch,
                            seq_to_sql(after_seq)?,
                            seq_to_sql(through_seq)?,
                            i64::try_from(limit).unwrap_or(i64::MAX),
                        ],
                        map_event_row,
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(rows)
            })
            .await
    }

    async fn oldest_seq(&self) -> AppResult<Option<u64>> {
        self.db
            .run(move |conn| {
                let seq: Option<i64> = conn
                    .query_row("SELECT MIN(seq) FROM remote_event_log", [], |row| {
                        row.get(0)
                    })
                    .map_err(|error| AppError::Database(error.to_string()))?;
                seq.map(seq_from_sql).transpose()
            })
            .await
    }

    async fn prune(&self, request: RemotePruneRequest) -> AppResult<RemotePruneOutcome> {
        // Compute the ceiling once in a plain autocommit read. It is deterministic given the
        // request plus the current log, and deleting rows below it can only *lower* the count of
        // eligible rows, never change which rows are eligible: appended rows carry seqs above
        // `max_seq`, and `created_at`-based aging only ever admits more rows, so recomputing per
        // chunk could not move the ceiling. Holding no write lock here is the point — the ceiling
        // read must not itself become the long transaction this method exists to avoid.
        let retention = RetentionParams {
            max_seq: request.max_seq,
            epoch_floor_seq: request.epoch_floor_seq,
            lease_floor: request.lease_floor,
            retain_rows: request.retain_rows,
            retain_days: request.retain_days,
        };
        let ceiling = self
            .db
            .run(move |conn| compute_prune_ceiling(conn, retention))
            .await?;
        let ceiling_sql = seq_to_sql(ceiling)?;

        // Chunked delete: each chunk is its own `BEGIN IMMEDIATE`/`COMMIT`, so the connection is
        // released between chunks and contended callers (auth reads, audit writes) interleave. The
        // safety predicate is unchanged — `seq <= ceiling`, where `ceiling <= lease_floor` by
        // construction — only the delete is bounded to `PRUNE_CHUNK_SIZE` rows per transaction.
        // Bundled SQLite is not built with `SQLITE_ENABLE_UPDATE_DELETE_LIMIT`, so `DELETE … LIMIT`
        // is unavailable; the portable bound is a `seq IN (SELECT … LIMIT)` sub-select over the
        // seq primary-key index.
        let mut deleted_total: u64 = 0;
        loop {
            let deleted = self
                .db
                .run_transaction(move |conn| {
                    let deleted = conn
                        .execute(
                            "DELETE FROM remote_event_log
                             WHERE seq IN (
                                 SELECT seq FROM remote_event_log
                                 WHERE seq <= ?1
                                 ORDER BY seq
                                 LIMIT ?2
                             )",
                            rusqlite::params![ceiling_sql, PRUNE_CHUNK_SIZE as i64],
                        )
                        .map_err(|error| AppError::Database(error.to_string()))?;
                    Ok(deleted as u64)
                })
                .await?;
            deleted_total += deleted;
            // A short chunk means the eligible span is exhausted; stop looping.
            if deleted < PRUNE_CHUNK_SIZE {
                break;
            }
        }

        Ok(RemotePruneOutcome {
            deleted: deleted_total,
            pruned_floor: ceiling,
        })
    }
}

/// The inputs to the ceiling computation, lifted out of [`RemotePruneRequest`] so the read closure
/// can own them by value without moving the whole request (whose `current_epoch` the ceiling does
/// not need).
#[derive(Debug, Clone, Copy)]
struct RetentionParams {
    max_seq: u64,
    epoch_floor_seq: u64,
    lease_floor: u64,
    retain_rows: u64,
    retain_days: i64,
}

/// Computes the highest seq the pruner may delete, reading only the age ceiling from the log.
fn compute_prune_ceiling(conn: &Connection, params: RetentionParams) -> AppResult<u64> {
    let cutoff = age_cutoff(params.retain_days);
    let age_ceiling: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM remote_event_log WHERE created_at < ?1",
            rusqlite::params![cutoff],
            |row| row.get(0),
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(prune_ceiling(
        retention_ceiling(
            params.max_seq,
            params.retain_rows,
            seq_from_sql(age_ceiling)?,
        ),
        params.epoch_floor_seq,
        params.lease_floor,
    ))
}

fn map_event_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RemoteEventRow> {
    Ok(RemoteEventRow {
        seq: row.get::<_, i64>(0)?.max(0) as u64,
        epoch: row.get(1)?,
        name: row.get(2)?,
        payload: row.get(3)?,
    })
}

fn read_high_water(conn: &Connection) -> AppResult<u64> {
    let stored: Option<i64> = match conn.query_row(
        "SELECT event_seq_high_water FROM remote_host_settings WHERE id = ?1",
        [SETTINGS_ROW_ID],
        |row| row.get(0),
    ) {
        Ok(value) => Some(value),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(error) => return Err(AppError::Database(error.to_string())),
    };
    stored
        .map(seq_from_sql)
        .transpose()
        .map(|seq| seq.unwrap_or(0))
}

/// SQLite has no unsigned integers; a seq that cannot round-trip is a bug, not a value to clamp.
fn seq_to_sql(seq: u64) -> AppResult<i64> {
    i64::try_from(seq).map_err(|_| AppError::Database(format!("remote event seq {seq} overflows")))
}

fn seq_from_sql(seq: i64) -> AppResult<u64> {
    u64::try_from(seq)
        .map_err(|_| AppError::Database(format!("remote event seq {seq} is negative")))
}

/// Matches the `strftime('%Y-%m-%dT%H:%M:%fZ','now')` column default so the comparison is
/// lexicographic over the same fixed-width shape.
fn age_cutoff(retain_days: i64) -> String {
    (chrono::Utc::now() - chrono::Duration::days(retain_days))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

#[cfg(test)]
#[path = "sqlite_remote_event_log_repo_tests.rs"]
mod tests;
