// Database connection management for SQLite

use rusqlite::Connection;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// Get the default development database path inside the repo.
pub fn get_default_db_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ralphx.db")
}

/// Get the database path inside the app data directory
pub fn get_app_data_db_path(app_data_dir: &Path) -> AppResult<PathBuf> {
    std::fs::create_dir_all(app_data_dir)
        .map_err(|e| AppError::Infrastructure(format!("Failed to create app data dir: {}", e)))?;

    Ok(app_data_dir.join("ralphx.db"))
}

/// Configure a SQLite connection with WAL mode and performance PRAGMAs.
///
/// Sets WAL journal mode (verified — warns if filesystem silently falls back),
/// busy_timeout=30000ms, and synchronous=NORMAL.
///
/// Must be called BEFORE run_migrations() so migrations run with WAL active.
///
/// # Errors
///
/// Returns `AppError::Database` if any PRAGMA fails.
pub fn configure_connection(conn: &Connection) -> AppResult<()> {
    // Set WAL mode and verify it actually activated (network filesystems may silently fall back)
    let journal_mode: String = conn
        .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
        .map_err(|e| AppError::Database(format!("Failed to set journal_mode: {e}")))?;

    if journal_mode.to_lowercase() != "wal" {
        tracing::warn!(
            actual_mode = %journal_mode,
            "SQLite WAL mode not activated — may be on unsupported filesystem. \
             Falling back to '{}' mode, which has reader/writer contention.",
            journal_mode
        );
    }

    conn.pragma_update(None, "busy_timeout", 30000)
        .map_err(|e| AppError::Database(format!("Failed to set busy_timeout: {e}")))?;

    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| AppError::Database(format!("Failed to set synchronous: {e}")))?;

    Ok(())
}

/// Open a database connection at the specified path.
///
/// Applies WAL mode PRAGMAs via `configure_connection()` before returning.
/// The caller is responsible for running migrations after this returns.
///
/// Creates the database file if it doesn't exist.
///
/// # Errors
///
/// Returns `AppError::Database` if the connection or PRAGMA setup fails.
pub fn open_connection(path: &PathBuf) -> AppResult<Connection> {
    let conn = Connection::open(path).map_err(|e| AppError::Database(e.to_string()))?;
    configure_connection(&conn)?;
    Ok(conn)
}

/// Open an in-memory database for testing.
///
/// Note: WAL mode is NOT applied to in-memory connections (SQLite does not support
/// WAL on in-memory databases). Tests that require WAL must use a file-based connection.
pub fn open_memory_connection() -> AppResult<Connection> {
    Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod tests;
