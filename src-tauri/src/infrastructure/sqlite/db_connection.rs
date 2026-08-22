// Shared DbConnection wrapper for executing blocking SQLite operations off tokio worker threads.
//
// All sqlite repo files should use DbConnection::run() for rusqlite calls to prevent
// blocking the tokio async runtime / timer driver.

#[cfg(test)]
#[path = "db_connection_tests.rs"]
mod tests;

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::{OnceLock, Weak};
use std::time::Instant;

use lazy_static::lazy_static;
use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};

use super::open_connection;

/// Newtype wrapper around a shared SQLite connection.
///
/// Provides `run()` and `query_optional()` methods that execute blocking rusqlite
/// operations on the tokio blocking thread pool via `spawn_blocking`. This prevents
/// rusqlite calls from blocking tokio worker threads, which would starve the timer
/// driver and make `tokio::time::timeout` unreliable.
///
/// # Usage
///
/// ```rust,ignore
/// let task = self.db.run(move |conn| {
///     conn.query_row(
///         "SELECT id, name FROM tasks WHERE id = ?1",
///         rusqlite::params![id.0],
///         |row| Ok(Task { id: row.get(0)?, name: row.get(1)? }),
///     )?;
///     Ok(task)
/// }).await?;
/// ```
#[derive(Clone)]
pub struct DbConnection {
    backend: Arc<DbBackend>,
}

enum DbBackend {
    Single(Arc<Mutex<Connection>>),
    Pool(ConnectionPool),
}

struct ConnectionPool {
    primary: Arc<Mutex<Connection>>,
    connections: Vec<Arc<Mutex<Connection>>>,
    next_index: AtomicUsize,
}

lazy_static! {
    static ref FILE_BACKED_POOLS: std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, Weak<DbBackend>>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
}

static STARTUP_BOOT_ID: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
struct DbLockTelemetryThresholds {
    wait_warn_ms: u128,
    hold_warn_ms: u128,
    warn_interval_ms: u128,
}

impl DbLockTelemetryThresholds {
    fn from_runtime() -> Self {
        let config = crate::infrastructure::agents::claude::stream_timeouts();
        Self {
            wait_warn_ms: u128::from(config.db_lock_wait_warn_ms),
            hold_warn_ms: u128::from(config.db_lock_hold_warn_ms),
            warn_interval_ms: u128::from(config.db_lock_warn_interval_ms),
        }
    }
}

/// Milliseconds since process start of the last emitted slow-lock WARN, plus how many have been
/// suppressed since. `u64::MAX` marks "never emitted" so the first slow lock is always reported.
static LAST_SLOW_LOCK_WARN_MS: AtomicU64 = AtomicU64::new(u64::MAX);
static SUPPRESSED_SLOW_LOCK_WARNS: AtomicU64 = AtomicU64::new(0);

fn process_uptime_ms() -> u64 {
    static PROCESS_START: OnceLock<Instant> = OnceLock::new();
    PROCESS_START
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX - 1)) as u64
}

/// Whether a slow-lock WARN should be emitted now, and how many were suppressed since the last
/// one. Pure in its inputs so it can be tested without waiting on a clock.
///
/// A contended database produces these faster than they can be read or written to disk — a
/// previous incident filled 41GB of logs in a day — so they are spaced out and the suppressed
/// count rides along on the next emitted line.
fn slow_lock_warn_decision(
    now_ms: u64,
    last_emit_ms: Option<u64>,
    warn_interval_ms: u128,
    suppressed_since_last: u64,
) -> Option<u64> {
    if warn_interval_ms == 0 {
        return Some(suppressed_since_last);
    }
    match last_emit_ms {
        // Never emitted, or a clock that appears to have gone backwards: report it.
        None => Some(suppressed_since_last),
        Some(last_emit_ms) => (u128::from(now_ms.saturating_sub(last_emit_ms)) >= warn_interval_ms)
            .then_some(suppressed_since_last),
    }
}

/// Applies the limiter against the process-wide counters.
fn claim_slow_lock_warn(warn_interval_ms: u128) -> Option<u64> {
    let now_ms = process_uptime_ms();
    let last_emit_ms = match LAST_SLOW_LOCK_WARN_MS.load(Ordering::Relaxed) {
        u64::MAX => None,
        value => Some(value),
    };
    let suppressed = SUPPRESSED_SLOW_LOCK_WARNS.load(Ordering::Relaxed);

    match slow_lock_warn_decision(now_ms, last_emit_ms, warn_interval_ms, suppressed) {
        Some(suppressed) => {
            LAST_SLOW_LOCK_WARN_MS.store(now_ms, Ordering::Relaxed);
            SUPPRESSED_SLOW_LOCK_WARNS.store(0, Ordering::Relaxed);
            Some(suppressed)
        }
        None => {
            SUPPRESSED_SLOW_LOCK_WARNS.fetch_add(1, Ordering::Relaxed);
            None
        }
    }
}

pub(crate) fn register_startup_boot_id(boot_id: &str) {
    let _ = STARTUP_BOOT_ID.set(boot_id.to_string());
}

fn startup_boot_id() -> &'static str {
    STARTUP_BOOT_ID
        .get()
        .map(String::as_str)
        .unwrap_or("unregistered")
}

fn db_caller_module(caller_file: &str) -> String {
    std::path::Path::new(caller_file)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn db_lock_observation_is_slow(
    lock_wait_ms: u128,
    lock_hold_ms: u128,
    thresholds: DbLockTelemetryThresholds,
) -> bool {
    lock_wait_ms >= thresholds.wait_warn_ms || lock_hold_ms >= thresholds.hold_warn_ms
}

#[allow(clippy::too_many_arguments)]
fn emit_db_lock_observation(
    lock_wait_ms: u128,
    lock_hold_ms: u128,
    method: &'static str,
    caller_module: &str,
    caller_line: u32,
    connection_backend: &'static str,
    connection_index: usize,
    connection_pick: &'static str,
    thresholds: DbLockTelemetryThresholds,
) {
    let boot_id = startup_boot_id();
    if db_lock_observation_is_slow(lock_wait_ms, lock_hold_ms, thresholds) {
        let Some(suppressed_count) = claim_slow_lock_warn(thresholds.warn_interval_ms) else {
            return;
        };
        tracing::warn!(
            target: "ralphx::db",
            boot_id,
            lock_wait_ms,
            lock_hold_ms,
            wait_warn_ms = thresholds.wait_warn_ms,
            hold_warn_ms = thresholds.hold_warn_ms,
            suppressed_count,
            method,
            caller_module,
            caller_line,
            connection_backend,
            connection_index,
            connection_pick,
            "Slow SQLite lock operation"
        );
    } else {
        tracing::debug!(
            target: "ralphx::db",
            boot_id,
            lock_wait_ms,
            lock_hold_ms,
            method,
            caller_module,
            caller_line,
            connection_backend,
            connection_index,
            connection_pick,
        );
    }
}

impl DbConnection {
    pub fn new(conn: Connection) -> Self {
        Self {
            backend: Arc::new(DbBackend::Single(Arc::new(Mutex::new(conn)))),
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        if let Some(path) = Self::file_backed_path(&conn) {
            if let Some(backend) = Self::pooled_backend(path, Arc::clone(&conn)) {
                return Self { backend };
            }
        }

        Self {
            backend: Arc::new(DbBackend::Single(conn)),
        }
    }

    /// Returns the inner Arc for legacy interop during migration.
    pub fn inner(&self) -> &Arc<Mutex<Connection>> {
        match self.backend.as_ref() {
            DbBackend::Single(conn) => conn,
            DbBackend::Pool(pool) => &pool.primary,
        }
    }

    /// Execute a blocking DB operation on the tokio blocking thread pool.
    ///
    /// The closure receives a `&Connection` and must return `AppResult<T>`.
    /// The `?` operator works on rusqlite errors inside the closure thanks to
    /// `impl From<rusqlite::Error> for AppError`.
    ///
    /// JoinError from `spawn_blocking` is mapped to `AppError::Database`.
    #[track_caller]
    pub fn run<F, T>(
        &self,
        f: F,
    ) -> impl std::future::Future<Output = AppResult<T>> + Send + 'static
    where
        F: FnOnce(&Connection) -> AppResult<T> + Send + 'static,
        T: Send + 'static,
    {
        let caller = std::panic::Location::caller();
        let caller_line = caller.line();
        let caller_module = db_caller_module(caller.file());
        let telemetry_thresholds = DbLockTelemetryThresholds::from_runtime();
        let (conn, connection_backend, connection_index, connection_pick) = self.pick_connection();
        async move {
            tokio::task::spawn_blocking(move || {
                let lock_start = std::time::Instant::now();

                let guard = conn.blocking_lock();

                let lock_acquired = std::time::Instant::now();

                let result = f(&guard);

                emit_db_lock_observation(
                    lock_acquired.duration_since(lock_start).as_millis(),
                    lock_acquired.elapsed().as_millis(),
                    "run",
                    &caller_module,
                    caller_line,
                    connection_backend,
                    connection_index,
                    connection_pick,
                    telemetry_thresholds,
                );

                result
            })
            .await
            .map_err(|e| AppError::Database(format!("spawn_blocking join error: {e}")))?
        }
    }

    /// Run a closure inside a SQLite transaction (BEGIN IMMEDIATE/COMMIT/ROLLBACK).
    ///
    /// Acquires the same `tokio::sync::Mutex` as `db.run()`. MUST NOT be called
    /// from within a `db.run()` closure — the tokio Mutex is non-reentrant and will
    /// deadlock immediately (caught in any test exercising the nested path).
    ///
    /// Events should be emitted AFTER this returns, outside the lock.
    ///
    /// # Errors
    ///
    /// Uses `BEGIN IMMEDIATE` so read-then-write transactions reserve the writer lock
    /// before any reads. This avoids SQLite WAL upgrade failures (`SQLITE_BUSY_SNAPSHOT`,
    /// surfaced as "database is locked") when another writer commits after a transaction's
    /// initial reads but before its first write.
    ///
    /// Returns `AppError::Database` on BEGIN/COMMIT failure or if the closure errors
    /// (which triggers automatic ROLLBACK).
    #[track_caller]
    pub fn run_transaction<F, T>(
        &self,
        f: F,
    ) -> impl std::future::Future<Output = AppResult<T>> + Send + 'static
    where
        F: FnOnce(&Connection) -> AppResult<T> + Send + 'static,
        T: Send + 'static,
    {
        let caller = std::panic::Location::caller();
        let caller_line = caller.line();
        let caller_module = db_caller_module(caller.file());
        let telemetry_thresholds = DbLockTelemetryThresholds::from_runtime();
        let (conn, connection_backend, connection_index, connection_pick) = self.pick_connection();
        async move {
            tokio::task::spawn_blocking(move || {
                let lock_start = std::time::Instant::now();

                let guard = conn.blocking_lock();

                let lock_acquired = std::time::Instant::now();

                guard
                    .execute_batch("BEGIN IMMEDIATE")
                    .map_err(|e| AppError::Database(format!("BEGIN IMMEDIATE failed: {e}")))?;
                let result = match f(&guard) {
                    Ok(result) => {
                        guard
                            .execute_batch("COMMIT")
                            .map_err(|e| AppError::Database(format!("COMMIT failed: {e}")))?;
                        Ok(result)
                    }
                    Err(e) => {
                        let _ = guard.execute_batch("ROLLBACK");
                        Err(e)
                    }
                };

                emit_db_lock_observation(
                    lock_acquired.duration_since(lock_start).as_millis(),
                    lock_acquired.elapsed().as_millis(),
                    "run_transaction",
                    &caller_module,
                    caller_line,
                    connection_backend,
                    connection_index,
                    connection_pick,
                    telemetry_thresholds,
                );

                result
            })
            .await
            .map_err(|e| AppError::Database(format!("spawn_blocking join error: {e}")))?
        }
    }

    /// Query that may return zero rows — maps `QueryReturnedNoRows` to `Ok(None)`.
    ///
    /// The closure receives a `&Connection` and should return `Result<T, rusqlite::Error>`.
    /// `QueryReturnedNoRows` is treated as `Ok(None)`, all other errors become
    /// `AppError::Database`.
    #[track_caller]
    pub fn query_optional<F, T>(
        &self,
        f: F,
    ) -> impl std::future::Future<Output = AppResult<Option<T>>> + Send + 'static
    where
        F: FnOnce(&Connection) -> Result<T, rusqlite::Error> + Send + 'static,
        T: Send + 'static,
    {
        self.run(move |conn| match f(conn) {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        })
    }

    fn pick_connection(&self) -> (Arc<Mutex<Connection>>, &'static str, usize, &'static str) {
        match self.backend.as_ref() {
            DbBackend::Single(conn) => (Arc::clone(conn), "single", 0, "single"),
            DbBackend::Pool(pool) => {
                let start =
                    pool.next_index.fetch_add(1, Ordering::Relaxed) % pool.connections.len();
                for offset in 0..pool.connections.len() {
                    let idx = (start + offset) % pool.connections.len();
                    if pool.connections[idx].try_lock().is_ok() {
                        return (
                            Arc::clone(&pool.connections[idx]),
                            "pool",
                            idx,
                            "first_available",
                        );
                    }
                }
                (
                    Arc::clone(&pool.connections[start]),
                    "pool",
                    start,
                    "round_robin",
                )
            }
        }
    }

    fn file_backed_path(conn: &Arc<Mutex<Connection>>) -> Option<std::path::PathBuf> {
        let guard = conn.try_lock().ok()?;
        let path: String = guard
            .query_row(
                "SELECT file FROM pragma_database_list WHERE name = 'main'",
                [],
                |row| row.get(0),
            )
            .ok()?;

        let path = path.trim();
        if path.is_empty() || path == ":memory:" || path.contains("mode=memory") {
            return None;
        }

        Some(std::path::PathBuf::from(path))
    }

    fn pooled_backend(
        path: std::path::PathBuf,
        primary: Arc<Mutex<Connection>>,
    ) -> Option<Arc<DbBackend>> {
        let mut cache = FILE_BACKED_POOLS.lock().ok()?;
        if let Some(existing) = cache.get(&path).and_then(Weak::upgrade) {
            return Some(existing);
        }

        match ConnectionPool::new(&path, Arc::clone(&primary)) {
            Ok(pool) => {
                let backend = Arc::new(DbBackend::Pool(pool));
                cache.insert(path, Arc::downgrade(&backend));
                Some(backend)
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Failed to create pooled SQLite backend; falling back to single connection"
                );
                None
            }
        }
    }
}

impl ConnectionPool {
    fn new(path: &std::path::PathBuf, primary: Arc<Mutex<Connection>>) -> AppResult<Self> {
        let pool_size = Self::pool_size();
        let mut connections = Vec::with_capacity(pool_size);
        connections.push(Arc::clone(&primary));

        for _ in 1..pool_size {
            let conn = open_connection(path)?;
            connections.push(Arc::new(Mutex::new(conn)));
        }

        tracing::info!(pool_size, "Initialized pooled SQLite backend");

        Ok(Self {
            primary,
            connections,
            next_index: AtomicUsize::new(0),
        })
    }

    fn pool_size() -> usize {
        const DEFAULT_POOL_SIZE: usize = 4;
        const MAX_POOL_SIZE: usize = 8;

        std::thread::available_parallelism()
            .map(|parallelism| parallelism.get().clamp(2, MAX_POOL_SIZE))
            .unwrap_or(DEFAULT_POOL_SIZE)
    }
}
