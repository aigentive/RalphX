//! Startup-only SQLite maintenance. This module must run before `DbConnection` is created.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;
use thiserror::Error;

pub const DEFAULT_AUTO_COMPACT_MAX_DB_BYTES: u64 = 2_147_483_648;
pub const DEFAULT_AUTO_COMPACT_MIN_FREELIST_PERCENT: u64 = 20;

#[derive(Debug, Clone, Copy)]
pub struct CompactionConfig {
    pub auto_enabled: bool,
    pub auto_max_db_bytes: u64,
    pub auto_min_freelist_percent: u64,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            auto_enabled: true,
            auto_max_db_bytes: DEFAULT_AUTO_COMPACT_MAX_DB_BYTES,
            auto_min_freelist_percent: DEFAULT_AUTO_COMPACT_MIN_FREELIST_PERCENT,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DatabaseMaintenanceStats {
    pub database_bytes: u64,
    pub reclaimable_bytes: u64,
    pub headroom_ok: bool,
    pub pending_compaction: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionOutcome {
    NotRequested,
    Skipped(&'static str),
    Compacted { reclaimed_bytes: u64 },
}

#[derive(Debug, Error)]
pub enum DatabaseMaintenanceError {
    #[error("database maintenance I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("database maintenance SQLite failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database integrity check failed: {0}")]
    Integrity(String),
}

/// App-owned maintenance paths. Production always derives these from
/// `AppPaths::database_maintenance_paths()` (process-owned data dir); tests
/// supply temp-dir equivalents so debug-profile database resolution can never
/// point maintenance at the shared dev database.
#[derive(Debug, Clone)]
pub struct MaintenancePaths {
    pub database_path: PathBuf,
    pub marker_path: PathBuf,
    pub backup_dir: PathBuf,
}

fn page_stats(conn: &Connection) -> Result<(u64, u64, u64), DatabaseMaintenanceError> {
    let page_size: u64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    let page_count: u64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let freelist_count: u64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
    Ok((page_size, page_count, freelist_count))
}

fn available_bytes(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: statvfs writes only the supplied initialized output struct and c_path is NUL-free.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: pointers remain valid for the call and are produced from valid Rust values.
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
            return None;
        }
        return (stat.f_bavail as u64).checked_mul(stat.f_frsize as u64);
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn backup_database(
    database_path: &Path,
    backup_dir: &Path,
) -> Result<u64, DatabaseMaintenanceError> {
    fs::create_dir_all(backup_dir)?;
    let backup_db = backup_dir.join("ralphx.db.pre-vacuum");
    let backup_wal = backup_dir.join("ralphx.db-wal.pre-vacuum");
    let db_bytes = fs::copy(database_path, &backup_db)?;
    if db_bytes == 0 || fs::metadata(&backup_db)?.len() != db_bytes {
        return Err(DatabaseMaintenanceError::Integrity(
            "backup database verification failed".into(),
        ));
    }
    let wal_path = PathBuf::from(format!("{}-wal", database_path.display()));
    if wal_path.exists() {
        let wal_bytes = fs::copy(&wal_path, &backup_wal)?;
        if fs::metadata(&backup_wal)?.len() != wal_bytes {
            return Err(DatabaseMaintenanceError::Integrity(
                "backup WAL verification failed".into(),
            ));
        }
        Ok(db_bytes.saturating_add(wal_bytes))
    } else {
        // A WAL backup from an earlier run must not survive next to a newer DB
        // backup: restoring that mismatched pair would replay unrelated WAL
        // frames into the restored database.
        if backup_wal.exists() {
            fs::remove_file(&backup_wal)?;
        }
        Ok(db_bytes)
    }
}

pub fn read_stats_at(
    paths: &MaintenancePaths,
) -> Result<DatabaseMaintenanceStats, DatabaseMaintenanceError> {
    if !paths.database_path.exists() {
        return Ok(DatabaseMaintenanceStats {
            database_bytes: 0,
            reclaimable_bytes: 0,
            headroom_ok: false,
            pending_compaction: paths.marker_path.exists(),
        });
    }
    let database_bytes = fs::metadata(&paths.database_path)?.len();
    // Stats are read at runtime while the pooled connection is live; open
    // read-only with a busy timeout so a concurrent writer cannot surface a
    // spurious SQLITE_BUSY in the Settings surface.
    let conn = Connection::open_with_flags(
        &paths.database_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let (page_size, _, freelist_count) = page_stats(&conn)?;
    let reclaimable_bytes = page_size.saturating_mul(freelist_count);
    let required = database_bytes.saturating_mul(3);
    let headroom_ok = available_bytes(
        paths
            .database_path
            .parent()
            .unwrap_or_else(|| Path::new(".")),
    )
    .is_some_and(|available| available >= required);
    Ok(DatabaseMaintenanceStats {
        database_bytes,
        reclaimable_bytes,
        headroom_ok,
        pending_compaction: paths.marker_path.exists(),
    })
}

pub fn set_pending_compaction_at(
    marker_path: &Path,
    pending: bool,
) -> Result<(), DatabaseMaintenanceError> {
    if pending {
        if let Some(parent) = marker_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(marker_path, b"compact on next launch\n")?;
    } else if marker_path.exists() {
        fs::remove_file(marker_path)?;
    }
    Ok(())
}

/// Consumes a pending manual request and, when eligible, vacuums before any pool is opened.
pub fn compact_before_pool_opens_at(
    paths: &MaintenancePaths,
    config: CompactionConfig,
) -> Result<CompactionOutcome, DatabaseMaintenanceError> {
    let manual = paths.marker_path.exists();
    if !manual && !config.auto_enabled {
        return Ok(CompactionOutcome::NotRequested);
    }
    if !paths.database_path.exists() {
        if manual {
            set_pending_compaction_at(&paths.marker_path, false)?;
        }
        return Ok(CompactionOutcome::Skipped("database_missing"));
    }
    let database_bytes = fs::metadata(&paths.database_path)?.len();
    let conn = Connection::open(&paths.database_path)?;
    let (_, page_count, freelist_count) = page_stats(&conn)?;
    let share_percent = if page_count == 0 {
        0
    } else {
        freelist_count.saturating_mul(100) / page_count
    };
    let required_headroom = database_bytes.saturating_mul(3);
    let available = available_bytes(
        paths
            .database_path
            .parent()
            .unwrap_or_else(|| Path::new(".")),
    );
    let skip = if available.is_none() {
        // Free-space probing is unsupported on this platform (non-unix) or
        // failed; fail closed but report it distinctly so a consumed manual
        // request is explainable from the logs.
        Some("disk_headroom_unavailable")
    } else if !available.is_some_and(|available| available >= required_headroom) {
        Some("insufficient_disk_headroom")
    } else if !manual && database_bytes > config.auto_max_db_bytes {
        Some("database_above_auto_limit")
    } else if !manual && share_percent < config.auto_min_freelist_percent {
        Some("freelist_below_auto_limit")
    } else {
        None
    };
    if let Some(reason) = skip {
        drop(conn);
        if manual {
            set_pending_compaction_at(&paths.marker_path, false)?;
        }
        return Ok(CompactionOutcome::Skipped(reason));
    }
    let backup_bytes = backup_database(&paths.database_path, &paths.backup_dir)?;
    // A verified backup exists before any destructive operation. Re-check headroom including it.
    if !available_bytes(
        paths
            .database_path
            .parent()
            .unwrap_or_else(|| Path::new(".")),
    )
    .is_some_and(|available| {
        available
            >= database_bytes
                .saturating_mul(2)
                .saturating_add(backup_bytes)
    }) {
        drop(conn);
        if manual {
            set_pending_compaction_at(&paths.marker_path, false)?;
        }
        return Ok(CompactionOutcome::Skipped("insufficient_disk_headroom"));
    }
    conn.execute_batch("VACUUM")?;
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(DatabaseMaintenanceError::Integrity(integrity));
    }
    drop(conn);
    let after = fs::metadata(&paths.database_path)?.len();
    if manual {
        set_pending_compaction_at(&paths.marker_path, false)?;
    }
    Ok(CompactionOutcome::Compacted {
        reclaimed_bytes: database_bytes.saturating_sub(after),
    })
}
