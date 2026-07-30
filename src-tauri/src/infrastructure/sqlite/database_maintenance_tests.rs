//! Tests for startup-only database maintenance (guarded compaction).
//!
//! All tests operate exclusively on temp-dir databases via `MaintenancePaths`;
//! they must never resolve paths through `AppPaths::database_path()`, which in
//! debug profiles points at the shared dev database.

use rusqlite::Connection;
use tempfile::TempDir;

use super::database_maintenance::{
    compact_before_pool_opens_at, read_stats_at, set_pending_compaction_at, CompactionConfig,
    CompactionOutcome, MaintenancePaths,
};

fn temp_paths(dir: &TempDir) -> MaintenancePaths {
    MaintenancePaths {
        database_path: dir.path().join("maintenance-test.db"),
        marker_path: dir.path().join("compact-on-next-launch"),
        backup_dir: dir.path().join("backups"),
    }
}

/// Creates a DB with a large deleted-row footprint so the freelist is non-trivial.
fn seed_bloated_db(paths: &MaintenancePaths) {
    let conn = Connection::open(&paths.database_path).unwrap();
    conn.execute_batch("CREATE TABLE payloads (id INTEGER PRIMARY KEY, body TEXT NOT NULL);")
        .unwrap();
    let blob = "x".repeat(4096);
    for chunk in 0..20 {
        let mut sql = String::from("INSERT INTO payloads (body) VALUES ");
        for i in 0..50 {
            if i > 0 {
                sql.push(',');
            }
            sql.push_str(&format!("('{}-{}-{}')", chunk, i, blob));
        }
        conn.execute_batch(&sql).unwrap();
    }
    conn.execute_batch("DELETE FROM payloads;").unwrap();
    drop(conn);
}

fn config(auto_enabled: bool) -> CompactionConfig {
    CompactionConfig {
        auto_enabled,
        auto_max_db_bytes: u64::MAX,
        auto_min_freelist_percent: 0,
    }
}

#[test]
fn not_requested_when_auto_disabled_and_no_marker() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_bloated_db(&paths);
    let outcome = compact_before_pool_opens_at(&paths, config(false)).unwrap();
    assert_eq!(outcome, CompactionOutcome::NotRequested);
}

#[test]
fn skips_and_consumes_marker_when_database_missing() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    set_pending_compaction_at(&paths.marker_path, true).unwrap();
    let outcome = compact_before_pool_opens_at(&paths, config(false)).unwrap();
    assert_eq!(outcome, CompactionOutcome::Skipped("database_missing"));
    assert!(
        !paths.marker_path.exists(),
        "marker must be consumed on skip"
    );
}

#[test]
fn auto_path_skips_database_above_size_limit() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_bloated_db(&paths);
    let outcome = compact_before_pool_opens_at(
        &paths,
        CompactionConfig {
            auto_enabled: true,
            auto_max_db_bytes: 1,
            auto_min_freelist_percent: 0,
        },
    )
    .unwrap();
    assert_eq!(
        outcome,
        CompactionOutcome::Skipped("database_above_auto_limit")
    );
}

#[test]
fn auto_path_skips_when_freelist_share_below_threshold() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_bloated_db(&paths);
    let outcome = compact_before_pool_opens_at(
        &paths,
        CompactionConfig {
            auto_enabled: true,
            auto_max_db_bytes: u64::MAX,
            auto_min_freelist_percent: 101,
        },
    )
    .unwrap();
    assert_eq!(
        outcome,
        CompactionOutcome::Skipped("freelist_below_auto_limit")
    );
}

#[test]
fn manual_marker_bypasses_auto_thresholds_and_compacts() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_bloated_db(&paths);
    let before = std::fs::metadata(&paths.database_path).unwrap().len();
    set_pending_compaction_at(&paths.marker_path, true).unwrap();

    // Thresholds would reject the auto path; the manual marker must bypass them.
    let outcome = compact_before_pool_opens_at(
        &paths,
        CompactionConfig {
            auto_enabled: false,
            auto_max_db_bytes: 1,
            auto_min_freelist_percent: 101,
        },
    )
    .unwrap();

    let after = std::fs::metadata(&paths.database_path).unwrap().len();
    match outcome {
        CompactionOutcome::Compacted { reclaimed_bytes } => {
            assert!(after < before, "vacuum must shrink the bloated database");
            assert_eq!(reclaimed_bytes, before - after);
        }
        other => panic!("expected compaction, got {other:?}"),
    }
    assert!(
        !paths.marker_path.exists(),
        "marker must be consumed on success"
    );
    assert!(
        paths.backup_dir.join("ralphx.db.pre-vacuum").exists(),
        "verified backup must exist before vacuum"
    );
    let conn = Connection::open(&paths.database_path).unwrap();
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}

#[test]
fn auto_path_compacts_bloated_database_within_limits() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_bloated_db(&paths);
    let before = std::fs::metadata(&paths.database_path).unwrap().len();
    let outcome = compact_before_pool_opens_at(&paths, config(true)).unwrap();
    match outcome {
        CompactionOutcome::Compacted { .. } => {
            let after = std::fs::metadata(&paths.database_path).unwrap().len();
            assert!(after < before);
        }
        other => panic!("expected compaction, got {other:?}"),
    }
}

#[test]
fn zero_byte_database_fails_backup_verification_and_keeps_marker() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    std::fs::write(&paths.database_path, b"").unwrap();
    set_pending_compaction_at(&paths.marker_path, true).unwrap();
    let result = compact_before_pool_opens_at(&paths, config(false));
    assert!(result.is_err(), "0-byte backup copy must abort compaction");
    assert!(
        paths.marker_path.exists(),
        "marker must survive a hard error so the request retries next launch"
    );
}

#[test]
fn read_stats_reports_reclaimable_freelist_bytes_and_pending_marker() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_bloated_db(&paths);

    let conn = Connection::open(&paths.database_path).unwrap();
    let page_size: u64 = conn
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .unwrap();
    let freelist: u64 = conn
        .query_row("PRAGMA freelist_count", [], |row| row.get(0))
        .unwrap();
    drop(conn);
    assert!(freelist > 0, "seed must produce free pages");

    let stats = read_stats_at(&paths).unwrap();
    assert_eq!(stats.reclaimable_bytes, page_size * freelist);
    assert_eq!(
        stats.database_bytes,
        std::fs::metadata(&paths.database_path).unwrap().len()
    );
    assert!(!stats.pending_compaction);

    set_pending_compaction_at(&paths.marker_path, true).unwrap();
    assert!(read_stats_at(&paths).unwrap().pending_compaction);
    set_pending_compaction_at(&paths.marker_path, false).unwrap();
    assert!(!read_stats_at(&paths).unwrap().pending_compaction);
}

#[test]
fn compaction_removes_a_stale_wal_backup_when_no_wal_exists() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_bloated_db(&paths);
    std::fs::create_dir_all(&paths.backup_dir).unwrap();
    let stale_wal_backup = paths.backup_dir.join("ralphx.db-wal.pre-vacuum");
    std::fs::write(&stale_wal_backup, b"stale wal frames from an older run").unwrap();

    let outcome = compact_before_pool_opens_at(&paths, config(true)).unwrap();

    assert!(matches!(outcome, CompactionOutcome::Compacted { .. }));
    assert!(
        !stale_wal_backup.exists(),
        "a stale WAL backup must not survive next to a newer DB backup"
    );
}

#[test]
fn read_stats_for_missing_database_is_empty_and_fail_closed_on_headroom() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    let stats = read_stats_at(&paths).unwrap();
    assert_eq!(stats.database_bytes, 0);
    assert_eq!(stats.reclaimable_bytes, 0);
    assert!(!stats.headroom_ok);
}

#[test]
fn set_pending_compaction_creates_parent_dirs() {
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("deeply").join("nested").join("marker");
    set_pending_compaction_at(&nested, true).unwrap();
    assert!(nested.exists());
    set_pending_compaction_at(&nested, false).unwrap();
    assert!(!nested.exists());
}

#[test]
fn set_pending_compaction_noop_when_clearing_absent_marker() {
    let dir = TempDir::new().unwrap();
    let marker = dir.path().join("nonexistent-marker");
    let result = set_pending_compaction_at(&marker, false);
    assert!(result.is_ok());
}

#[test]
fn read_stats_reports_headroom_ok_when_disk_has_space() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    let conn = Connection::open(&paths.database_path).unwrap();
    conn.execute_batch("CREATE TABLE tiny (id INTEGER PRIMARY KEY);")
        .unwrap();
    drop(conn);

    let stats = read_stats_at(&paths).unwrap();
    assert!(
        stats.headroom_ok,
        "a tiny database in a temp dir should have plenty of headroom"
    );
}

#[test]
fn read_stats_pending_reflects_missing_database_marker() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    set_pending_compaction_at(&paths.marker_path, true).unwrap();
    let stats = read_stats_at(&paths).unwrap();
    assert!(stats.pending_compaction);
    assert_eq!(stats.database_bytes, 0);
}
