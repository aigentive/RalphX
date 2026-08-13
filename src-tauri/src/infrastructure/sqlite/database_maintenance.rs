//! Startup-only SQLite maintenance. This module must run before `DbConnection` is created.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;
use thiserror::Error;

use super::database_maintenance_outcome::{
    read_record, write_record, CompactionRecord, REASON_SWAP_INTERRUPTED,
};

pub const DEFAULT_AUTO_COMPACT_MAX_DB_BYTES: u64 = 2_147_483_648;
pub const DEFAULT_AUTO_COMPACT_MIN_FREELIST_PERCENT: u64 = 20;

/// Safety margin over the estimated compacted size, as a divisor (5 → 20%).
const HEADROOM_SAFETY_DIVISOR: u64 = 5;

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
    /// Outcome of the most recent compaction attempt, including why it was skipped.
    pub last_compaction: Option<CompactionRecord>,
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

struct LabeledError {
    label: String,
    source: DatabaseMaintenanceError,
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
    pub outcome_path: PathBuf,
}

fn page_stats(conn: &Connection) -> Result<(u64, u64, u64), DatabaseMaintenanceError> {
    let page_size: u64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    let page_count: u64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let freelist_count: u64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
    Ok((page_size, page_count, freelist_count))
}

// statvfs field widths differ per platform (u32 on macOS, u64 on Linux), so
// the casts below are required even where clippy sees them as no-ops.
#[allow(clippy::unnecessary_cast)]
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
        (stat.f_bavail as u64).checked_mul(stat.f_frsize as u64)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn sidecar_path(database_path: &Path, suffix: &str) -> PathBuf {
    // Fixed suffixes on an app-owned database path — no user-influenced components.
    PathBuf::from(format!("{}{suffix}", database_path.display()))
}

fn file_len(path: &Path) -> u64 {
    fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// Free space a `VACUUM INTO` swap needs: the *compacted* size plus a 20% margin plus
/// the live WAL.
///
/// The old copy-backup + in-place `VACUUM` path required 3× the database size — 135 GB
/// free for a 45 GB database, which is why compaction was unreachable exactly where it
/// mattered most.
#[must_use]
pub fn required_headroom_bytes(database_bytes: u64, reclaimable_bytes: u64, wal_bytes: u64) -> u64 {
    let estimated_compacted = database_bytes.saturating_sub(reclaimable_bytes);
    estimated_compacted
        .saturating_add(estimated_compacted / HEADROOM_SAFETY_DIVISOR)
        .saturating_add(wal_bytes)
}

fn database_dir(database_path: &Path) -> &Path {
    database_path.parent().unwrap_or_else(|| Path::new("."))
}

pub fn read_stats_at(
    paths: &MaintenancePaths,
) -> Result<DatabaseMaintenanceStats, DatabaseMaintenanceError> {
    let last_compaction = read_record(&paths.outcome_path);
    if !paths.database_path.exists() {
        return Ok(DatabaseMaintenanceStats {
            database_bytes: 0,
            reclaimable_bytes: 0,
            headroom_ok: false,
            pending_compaction: paths.marker_path.exists(),
            last_compaction,
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
    let required = required_headroom_bytes(
        database_bytes,
        reclaimable_bytes,
        file_len(&sidecar_path(&paths.database_path, "-wal")),
    );
    let headroom_ok = available_bytes(database_dir(&paths.database_path))
        .is_some_and(|available| available >= required);
    Ok(DatabaseMaintenanceStats {
        database_bytes,
        reclaimable_bytes,
        headroom_ok,
        pending_compaction: paths.marker_path.exists(),
        last_compaction,
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

/// Whether a compaction will actually execute, so callers can advance a startup stage
/// only when the user is about to wait for one.
#[must_use]
pub fn compaction_will_execute(paths: &MaintenancePaths, config: CompactionConfig) -> bool {
    matches!(
        decide(paths, config),
        Ok(CompactionDecision::Execute { .. })
    )
}

enum CompactionDecision {
    NotRequested,
    Skipped {
        reason: &'static str,
        manual: bool,
        database_bytes: u64,
    },
    Execute {
        manual: bool,
        database_bytes: u64,
    },
}

fn decide(
    paths: &MaintenancePaths,
    config: CompactionConfig,
) -> Result<CompactionDecision, DatabaseMaintenanceError> {
    let manual = paths.marker_path.exists();
    if !manual && !config.auto_enabled {
        return Ok(CompactionDecision::NotRequested);
    }
    if !paths.database_path.exists() {
        return Ok(CompactionDecision::Skipped {
            reason: "database_missing",
            manual,
            database_bytes: 0,
        });
    }
    let database_bytes = fs::metadata(&paths.database_path)?.len();
    let conn = Connection::open_with_flags(
        &paths.database_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let (page_size, page_count, freelist_count) = page_stats(&conn)?;
    drop(conn);

    let share_percent = if page_count == 0 {
        0
    } else {
        freelist_count.saturating_mul(100) / page_count
    };
    let required_headroom = required_headroom_bytes(
        database_bytes,
        page_size.saturating_mul(freelist_count),
        file_len(&sidecar_path(&paths.database_path, "-wal")),
    );
    let available = available_bytes(database_dir(&paths.database_path));
    let reason = if available.is_none() {
        // Free-space probing is unsupported on this platform (non-unix) or
        // failed; fail closed but report it distinctly so a consumed manual
        // request is explainable from the sidecar.
        Some("disk_headroom_unavailable")
    } else if available.is_none_or(|available| available < required_headroom) {
        Some("insufficient_disk_headroom")
    } else if !manual && database_bytes > config.auto_max_db_bytes {
        Some("database_above_auto_limit")
    } else if !manual && share_percent < config.auto_min_freelist_percent {
        Some("freelist_below_auto_limit")
    } else {
        None
    };
    Ok(match reason {
        Some(reason) => CompactionDecision::Skipped {
            reason,
            manual,
            database_bytes,
        },
        None => CompactionDecision::Execute {
            manual,
            database_bytes,
        },
    })
}

fn compact_before_pool_opens_at_impl(
    paths: &MaintenancePaths,
    config: CompactionConfig,
    verifier: &dyn Fn(&Path) -> Result<(), DatabaseMaintenanceError>,
    pre_rename_hook: Option<&dyn Fn()>,
) -> Result<CompactionOutcome, DatabaseMaintenanceError> {
    let decision = match decide(paths, config) {
        Ok(decision) => decision,
        Err(error) => {
            // The marker deliberately survives a hard error so the request retries.
            write_record(
                &paths.outcome_path,
                &CompactionRecord::error(
                    file_len(&paths.database_path),
                    &format!("eligibility: {error}"),
                ),
            );
            return Err(error);
        }
    };
    match decision {
        CompactionDecision::NotRequested => Ok(CompactionOutcome::NotRequested),
        CompactionDecision::Skipped {
            reason,
            manual,
            database_bytes,
        } => {
            // Record before clearing the marker: a consumed manual request that explains
            // nothing is the bug this replaces.
            write_record(
                &paths.outcome_path,
                &CompactionRecord::skipped(database_bytes, reason),
            );
            if manual {
                set_pending_compaction_at(&paths.marker_path, false)?;
            }
            Ok(CompactionOutcome::Skipped(reason))
        }
        CompactionDecision::Execute {
            manual,
            database_bytes,
        } => {
            let result = vacuum_into_swap_impl(paths, database_bytes, verifier, pre_rename_hook);
            let outcome = match result {
                Ok(o) => {
                    match &o {
                        CompactionOutcome::Compacted { reclaimed_bytes } => write_record(
                            &paths.outcome_path,
                            &CompactionRecord::compacted(database_bytes, *reclaimed_bytes),
                        ),
                        CompactionOutcome::Skipped(reason) => write_record(
                            &paths.outcome_path,
                            &CompactionRecord::skipped(database_bytes, reason),
                        ),
                        CompactionOutcome::NotRequested => {}
                    }
                    Ok(o)
                }
                Err(le) => {
                    write_record(
                        &paths.outcome_path,
                        &CompactionRecord::error(database_bytes, &le.label),
                    );
                    Err(le.source)
                }
            };
            if manual {
                set_pending_compaction_at(&paths.marker_path, false)?;
            }
            outcome
        }
    }
}

/// Consumes a pending manual request and, when eligible, compacts before any pool is opened.
///
/// Compacts into a new file, verifies it, then swaps it in — the untouched original *is*
/// the backup until the swap completes, so no copy is needed.
pub fn compact_before_pool_opens_at(
    paths: &MaintenancePaths,
    config: CompactionConfig,
) -> Result<CompactionOutcome, DatabaseMaintenanceError> {
    compact_before_pool_opens_at_impl(paths, config, &verify_replacement, None)
}

/// Test seam: threads a custom verifier and an optional pre-rename hook through the swap
/// so obligations 6(b) and 6(c) can be proven without depending on filesystem corruption.
#[cfg(test)]
pub(super) fn compact_before_pool_opens_at_with_seams(
    paths: &MaintenancePaths,
    config: CompactionConfig,
    verifier: &dyn Fn(&Path) -> Result<(), DatabaseMaintenanceError>,
    pre_rename_hook: Option<&dyn Fn()>,
) -> Result<CompactionOutcome, DatabaseMaintenanceError> {
    compact_before_pool_opens_at_impl(paths, config, verifier, pre_rename_hook)
}

fn vacuum_into_swap_impl(
    paths: &MaintenancePaths,
    database_bytes: u64,
    verifier: &dyn Fn(&Path) -> Result<(), DatabaseMaintenanceError>,
    pre_rename_hook: Option<&dyn Fn()>,
) -> Result<CompactionOutcome, LabeledError> {
    let compacting_path = sidecar_path(&paths.database_path, ".compacting");
    let wal_path = sidecar_path(&paths.database_path, "-wal");
    let shm_path = sidecar_path(&paths.database_path, "-shm");

    let conn = Connection::open(&paths.database_path).map_err(|e| {
        let label = format!("open: {e}");
        LabeledError { label, source: e.into() }
    })?;
    // (a) The one precondition that may NOT be best-effort: a surviving WAL would be
    // discarded by the swap below, losing committed data.
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
        .map_err(|e| {
            let label = format!("checkpoint_query: {e}");
            LabeledError { label, source: e.into() }
        })?;
    if busy != 0 || file_len(&wal_path) > 0 {
        drop(conn);
        return Ok(CompactionOutcome::Skipped("wal_checkpoint_incomplete"));
    }

    // (b) Compact into a fresh file. The live database is untouched.
    if compacting_path.exists() {
        fs::remove_file(&compacting_path).map_err(|e| {
            let label = format!("pre_vacuum_cleanup: {e}");
            LabeledError { label, source: e.into() }
        })?;
    }
    if let Err(error) = conn.execute("VACUUM INTO ?1", [compacting_path.to_string_lossy()]) {
        drop(conn);
        let _ = fs::remove_file(&compacting_path);
        let label = format!("vacuum: {error}");
        return Err(LabeledError { label, source: error.into() });
    }
    // (c) Verify the replacement before anything destructive happens.
    drop(conn);
    if let Err(error) = verifier(&compacting_path) {
        let _ = fs::remove_file(&compacting_path);
        let label = format!("verify: {error}");
        return Err(LabeledError { label, source: error });
    }

    // (d) The original becomes the backup by moving, not copying.
    fs::create_dir_all(&paths.backup_dir).map_err(|e| {
        let label = format!("backup_dir_create: {e}");
        LabeledError { label, source: e.into() }
    })?;
    let backup_path = paths.backup_dir.join("ralphx.db.pre-vacuum");
    // A WAL backup written by an earlier release must not survive beside a newer DB
    // backup: restoring that mismatched pair would replay unrelated WAL frames.
    let _ = fs::remove_file(paths.backup_dir.join("ralphx.db-wal.pre-vacuum"));
    // From the next line until the rename-in at (f) the live path holds no database. Dying
    // in that window would otherwise leave the *previous* run's record standing — usually
    // `compacted` — describing a healthy database that is no longer there. Best-effort like
    // every other sidecar write: a failed breadcrumb must never block startup.
    write_record(
        &paths.outcome_path,
        &CompactionRecord::error(database_bytes, REASON_SWAP_INTERRUPTED),
    );
    fs::rename(&paths.database_path, &backup_path).map_err(|e| {
        let label = format!("backup_rename: {e}");
        LabeledError { label, source: e.into() }
    })?;

    // (e) The emptied WAL/SHM belong to the file that just moved out. Removing them
    // before the rename-in avoids a crash window where a compacted database sits
    // beside a foreign WAL.
    let _ = fs::remove_file(&wal_path);
    let _ = fs::remove_file(&shm_path);

    // Optional hook for obligation-6(c) tests: fires between (e) and (f) so a test
    // can inject a condition that makes the rename-in fail without filesystem corruption.
    if let Some(hook) = pre_rename_hook {
        hook();
    }

    // (f) Swap the verified replacement in; on failure put the original straight back.
    if let Err(error) = fs::rename(&compacting_path, &paths.database_path) {
        // The restore decides where the database actually ended up, so its result — not an
        // assumption — has to drive the recorded location.
        let label = match fs::rename(&backup_path, &paths.database_path) {
            Ok(()) => format!(
                "swap_rename: {error} (original restored to {})",
                paths.database_path.display()
            ),
            Err(restore_error) => format!(
                "swap_rename: {error} (restore failed: {restore_error}; original preserved at {})",
                backup_path.display()
            ),
        };
        return Err(LabeledError { label, source: error.into() });
    }

    Ok(CompactionOutcome::Compacted {
        reclaimed_bytes: database_bytes.saturating_sub(file_len(&paths.database_path)),
    })
}

fn verify_replacement(compacting_path: &Path) -> Result<(), DatabaseMaintenanceError> {
    let replacement = Connection::open(compacting_path)?;
    let integrity: String =
        replacement.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    drop(replacement);
    if integrity == "ok" {
        Ok(())
    } else {
        Err(DatabaseMaintenanceError::Integrity(integrity))
    }
}
