// Shared DbConnection wrapper for executing blocking SQLite operations off tokio worker threads.
//
// All sqlite repo files should use DbConnection::run() for rusqlite calls to prevent
// blocking the tokio async runtime / timer driver.

#[cfg(test)]
#[path = "db_connection_tests.rs"]
mod tests;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Weak;

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
        let caller_file = caller.file().to_string();
        let caller_line = caller.line();
        let (conn, connection_backend, connection_index) = self.pick_connection();
        async move {
            tokio::task::spawn_blocking(move || {
                #[cfg(debug_assertions)]
                let lock_start = std::time::Instant::now();

                let guard = conn.blocking_lock();

                #[cfg(debug_assertions)]
                let lock_acquired = std::time::Instant::now();

                let result = f(&guard);

                #[cfg(debug_assertions)]
                {
                    let lock_wait_ms = lock_acquired.duration_since(lock_start).as_millis();
                    let lock_hold_ms = lock_acquired.elapsed().as_millis();
                    if lock_wait_ms > 100 {
                        tracing::warn!(
                            target: "ralphx::db",
                            lock_wait_ms,
                            lock_hold_ms,
                            method = "run",
                            caller_file = caller_file.as_str(),
                            caller_line,
                            connection_backend,
                            connection_index,
                            "DB lock contention: lock wait exceeded 100ms"
                        );
                    } else {
                        tracing::debug!(
                            target: "ralphx::db",
                            lock_wait_ms,
                            lock_hold_ms,
                            method = "run",
                            caller_file = caller_file.as_str(),
                            caller_line,
                            connection_backend,
                            connection_index,
                        );
                    }
                }

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
        let caller_file = caller.file().to_string();
        let caller_line = caller.line();
        let (conn, connection_backend, connection_index) = self.pick_connection();
        async move {
            tokio::task::spawn_blocking(move || {
                #[cfg(debug_assertions)]
                let lock_start = std::time::Instant::now();

                let guard = conn.blocking_lock();

                #[cfg(debug_assertions)]
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

                #[cfg(debug_assertions)]
                {
                    let lock_wait_ms = lock_acquired.duration_since(lock_start).as_millis();
                    let lock_hold_ms = lock_acquired.elapsed().as_millis();
                    if lock_wait_ms > 100 {
                        tracing::warn!(
                            target: "ralphx::db",
                            lock_wait_ms,
                            lock_hold_ms,
                            method = "run_transaction",
                            caller_file = caller_file.as_str(),
                            caller_line,
                            connection_backend,
                            connection_index,
                            "DB lock contention: lock wait exceeded 100ms"
                        );
                    } else {
                        tracing::debug!(
                            target: "ralphx::db",
                            lock_wait_ms,
                            lock_hold_ms,
                            method = "run_transaction",
                            caller_file = caller_file.as_str(),
                            caller_line,
                            connection_backend,
                            connection_index,
                        );
                    }
                }

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

    fn pick_connection(&self) -> (Arc<Mutex<Connection>>, &'static str, usize) {
        match self.backend.as_ref() {
            DbBackend::Single(conn) => (Arc::clone(conn), "single", 0),
            DbBackend::Pool(pool) => {
                let idx = pool.next_index.fetch_add(1, Ordering::Relaxed) % pool.connections.len();
                (Arc::clone(&pool.connections[idx]), "pool", idx)
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

        tracing::info!(
            path = %path.display(),
            pool_size,
            "Initialized pooled SQLite backend"
        );

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
