//! Tests for startup-only database maintenance (guarded compaction).
//!
//! All tests operate exclusively on temp-dir databases via `MaintenancePaths`;
//! they must never resolve paths through `AppPaths::database_path()`, which in
//! debug profiles points at the shared dev database.

use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;
use tempfile::TempDir;

use super::database_maintenance::{
    compact_before_pool_opens_at, compact_before_pool_opens_at_with_seams, compaction_will_execute,
    read_stats_at, required_headroom_bytes, set_pending_compaction_at, CompactionConfig,
    CompactionOutcome, DatabaseMaintenanceError, DatabaseMaintenanceStats, MaintenancePaths,
    DEFAULT_AUTO_COMPACT_MAX_DB_BYTES, DEFAULT_AUTO_COMPACT_MIN_FREELIST_PERCENT,
};
use super::database_maintenance_outcome::{
    read_record, write_record, CompactionRecord, COMPACTION_OUTCOME_FILE_NAME, OUTCOME_COMPACTED,
    OUTCOME_ERROR, OUTCOME_SKIPPED, REASON_SWAP_INTERRUPTED,
};

fn temp_paths(dir: &TempDir) -> MaintenancePaths {
    MaintenancePaths {
        database_path: dir.path().join("maintenance-test.db"),
        marker_path: dir.path().join("compact-on-next-launch"),
        backup_dir: dir.path().join("backups"),
        outcome_path: dir.path().join(COMPACTION_OUTCOME_FILE_NAME),
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

/// Seeds a genuine WAL-mode database with live rows, then leaves the WAL populated.
fn seed_wal_mode_db(paths: &MaintenancePaths) {
    let conn = Connection::open(&paths.database_path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.execute_batch("CREATE TABLE payloads (id INTEGER PRIMARY KEY, body TEXT NOT NULL);")
        .unwrap();
    let blob = "x".repeat(4096);
    for chunk in 0..10 {
        let mut sql = String::from("INSERT INTO payloads (body) VALUES ");
        for i in 0..50 {
            if i > 0 {
                sql.push(',');
            }
            sql.push_str(&format!("('{}-{}-{}')", chunk, i, blob));
        }
        conn.execute_batch(&sql).unwrap();
    }
    conn.execute_batch("DELETE FROM payloads WHERE id % 2 = 0;")
        .unwrap();
    drop(conn);
}

fn wal_path(paths: &MaintenancePaths) -> PathBuf {
    PathBuf::from(format!("{}-wal", paths.database_path.display()))
}

fn shm_path(paths: &MaintenancePaths) -> PathBuf {
    PathBuf::from(format!("{}-shm", paths.database_path.display()))
}

fn config(auto_enabled: bool) -> CompactionConfig {
    CompactionConfig {
        auto_enabled,
        auto_max_db_bytes: u64::MAX,
        auto_min_freelist_percent: 0,
    }
}

#[test]
fn compaction_config_default_uses_published_constants() {
    let default = CompactionConfig::default();
    assert!(default.auto_enabled);
    assert_eq!(default.auto_max_db_bytes, DEFAULT_AUTO_COMPACT_MAX_DB_BYTES);
    assert_eq!(
        default.auto_min_freelist_percent,
        DEFAULT_AUTO_COMPACT_MIN_FREELIST_PERCENT
    );
}

#[test]
fn database_maintenance_stats_serializes_to_json() {
    let stats = DatabaseMaintenanceStats {
        database_bytes: 1024,
        reclaimable_bytes: 256,
        headroom_ok: true,
        pending_compaction: false,
        last_compaction: Some(CompactionRecord::skipped(
            1024,
            "insufficient_disk_headroom",
        )),
    };
    let json = serde_json::to_value(&stats).unwrap();
    assert_eq!(json["database_bytes"], 1024);
    assert_eq!(json["reclaimable_bytes"], 256);
    assert_eq!(json["headroom_ok"], true);
    assert_eq!(json["pending_compaction"], false);
    assert_eq!(json["last_compaction"]["outcome"], OUTCOME_SKIPPED);
    assert_eq!(
        json["last_compaction"]["reason"],
        "insufficient_disk_headroom"
    );
}

#[test]
fn headroom_needs_the_compacted_size_plus_a_margin_not_three_times_the_file() {
    // The incident database: 45 GB with 35 GB reclaimable.
    let database_bytes = 45 * 1024 * 1024 * 1024_u64;
    let reclaimable_bytes = 35 * 1024 * 1024 * 1024_u64;

    let required = required_headroom_bytes(database_bytes, reclaimable_bytes, 0);

    let compacted = database_bytes - reclaimable_bytes;
    assert_eq!(required, compacted + compacted / 5);
    assert!(
        required < 13 * 1024 * 1024 * 1024,
        "≈12 GB, not the 135 GB the copy-backup path demanded (got {required})"
    );
}

#[test]
fn headroom_accounts_for_a_live_wal() {
    assert_eq!(required_headroom_bytes(1_000, 0, 500), 1_000 + 200 + 500);
}

#[test]
fn not_requested_when_auto_disabled_and_no_marker() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_bloated_db(&paths);
    let outcome = compact_before_pool_opens_at(&paths, config(false)).unwrap();
    assert_eq!(outcome, CompactionOutcome::NotRequested);
    assert!(!compaction_will_execute(&paths, config(false)));
}

#[test]
fn skips_and_consumes_marker_when_database_missing_but_records_the_reason_first() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    set_pending_compaction_at(&paths.marker_path, true).unwrap();
    let outcome = compact_before_pool_opens_at(&paths, config(false)).unwrap();
    assert_eq!(outcome, CompactionOutcome::Skipped("database_missing"));
    assert!(
        !paths.marker_path.exists(),
        "marker must be consumed on skip"
    );
    let record = read_record(&paths.outcome_path).expect("skip must be recorded");
    assert_eq!(record.outcome, OUTCOME_SKIPPED);
    assert_eq!(record.reason.as_deref(), Some("database_missing"));
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
fn manual_marker_bypasses_auto_thresholds_and_compacts_into_a_verified_replacement() {
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
    let CompactionOutcome::Compacted { reclaimed_bytes } = outcome else {
        panic!("expected CompactionOutcome::Compacted variant");
    };
    assert!(
        after < before,
        "compaction must shrink the bloated database"
    );
    assert_eq!(reclaimed_bytes, before - after);
    assert!(
        !paths.marker_path.exists(),
        "marker must be consumed on success"
    );
    assert!(
        paths.backup_dir.join("ralphx.db.pre-vacuum").exists(),
        "the original must survive as the backup"
    );
    assert!(
        !PathBuf::from(format!("{}.compacting", paths.database_path.display())).exists(),
        "the scratch replacement must not linger"
    );
    let conn = Connection::open(&paths.database_path).unwrap();
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    drop(conn);

    let record = read_record(&paths.outcome_path).expect("success must be recorded");
    assert_eq!(record.outcome, OUTCOME_COMPACTED);
    assert_eq!(record.reclaimed_bytes, Some(reclaimed_bytes));
    assert_eq!(record.database_bytes_before, before);
}

#[test]
fn compaction_preserves_the_data_it_compacts() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    let conn = Connection::open(&paths.database_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE keep (id INTEGER PRIMARY KEY, body TEXT NOT NULL);
         INSERT INTO keep (body) VALUES ('first'), ('second'), ('third');",
    )
    .unwrap();
    drop(conn);

    // Manual marker bypasses auto-thresholds (the tiny table has no freelist).
    set_pending_compaction_at(&paths.marker_path, true).unwrap();
    let outcome = compact_before_pool_opens_at(&paths, config(false)).unwrap();
    assert!(
        matches!(outcome, CompactionOutcome::Compacted { .. }),
        "manual marker must force compaction, got {outcome:?}"
    );

    let conn = Connection::open(&paths.database_path).unwrap();
    let mut stmt = conn
        .prepare("SELECT body FROM keep ORDER BY id")
        .unwrap();
    let bodies: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(
        bodies,
        ["first", "second", "third"],
        "the swapped-in database must be data-equivalent to the original"
    );
}

#[test]
fn auto_path_compacts_bloated_database_within_limits() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_bloated_db(&paths);
    let before = std::fs::metadata(&paths.database_path).unwrap().len();
    assert!(compaction_will_execute(&paths, config(true)));
    let outcome = compact_before_pool_opens_at(&paths, config(true)).unwrap();
    assert!(
        matches!(outcome, CompactionOutcome::Compacted { .. }),
        "expected CompactionOutcome::Compacted variant"
    );
    let after = std::fs::metadata(&paths.database_path).unwrap().len();
    assert!(after < before);
}

#[test]
fn a_wal_mode_database_compacts_and_leaves_no_orphaned_wal_beside_it() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_wal_mode_db(&paths);

    let outcome = compact_before_pool_opens_at(&paths, config(true)).unwrap();

    assert!(matches!(outcome, CompactionOutcome::Compacted { .. }));
    assert!(
        !wal_path(&paths).exists(),
        "the WAL belonged to the file that moved out"
    );
    assert!(!shm_path(&paths).exists(), "no orphaned shared-memory file");
    let conn = Connection::open(&paths.database_path).unwrap();
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM payloads", [], |row| row.get(0))
        .unwrap();
    assert!(rows > 0, "surviving rows must still be readable");
}

#[test]
fn an_uncheckpointable_wal_aborts_before_anything_destructive_happens() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_wal_mode_db(&paths);

    // Keep WAL frames live, then hold a read snapshot open: a TRUNCATE checkpoint
    // cannot complete, and the frames it leaves behind would be lost in the swap.
    let writer = Connection::open(&paths.database_path).unwrap();
    writer
        .execute_batch("INSERT INTO payloads (body) VALUES ('uncheckpointed');")
        .unwrap();
    let reader = Connection::open(&paths.database_path).unwrap();
    reader.execute_batch("BEGIN").unwrap();
    let _held: i64 = reader
        .query_row("SELECT COUNT(*) FROM payloads", [], |row| row.get(0))
        .unwrap();
    assert!(wal_path(&paths).exists(), "fixture must leave a live WAL");

    let outcome = compact_before_pool_opens_at(&paths, config(true)).unwrap();

    assert_eq!(
        outcome,
        CompactionOutcome::Skipped("wal_checkpoint_incomplete")
    );
    assert!(
        !paths.backup_dir.join("ralphx.db.pre-vacuum").exists(),
        "nothing destructive may happen before the checkpoint succeeds"
    );
    assert!(wal_path(&paths).exists(), "the WAL must not be deleted");
    let record = read_record(&paths.outcome_path).expect("abort must be recorded");
    assert_eq!(record.reason.as_deref(), Some("wal_checkpoint_incomplete"));

    reader.execute_batch("ROLLBACK").unwrap();
    let rows: i64 = writer
        .query_row("SELECT COUNT(*) FROM payloads", [], |row| row.get(0))
        .unwrap();
    assert!(rows > 0, "the uncheckpointed rows survive");
}

#[test]
fn a_corrupt_database_errors_records_the_phase_and_keeps_the_marker() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    std::fs::write(&paths.database_path, b"this is definitely not a database").unwrap();
    set_pending_compaction_at(&paths.marker_path, true).unwrap();

    let result = compact_before_pool_opens_at(&paths, config(false));

    assert!(result.is_err(), "an unreadable database must abort");
    assert!(
        paths.marker_path.exists(),
        "marker must survive a hard error so the request retries next launch"
    );
    let record = read_record(&paths.outcome_path).expect("error must be recorded");
    assert_eq!(record.outcome, OUTCOME_ERROR);
    assert!(
        record.reason.is_some(),
        "the failing phase must be recorded"
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
fn read_stats_round_trips_the_last_compaction_record() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_bloated_db(&paths);

    assert!(read_stats_at(&paths).unwrap().last_compaction.is_none());
    compact_before_pool_opens_at(&paths, config(true)).unwrap();

    let record = read_stats_at(&paths)
        .unwrap()
        .last_compaction
        .expect("Settings must be able to read the last outcome");
    assert_eq!(record.outcome, OUTCOME_COMPACTED);
}

#[test]
fn a_corrupt_outcome_sidecar_reads_as_no_record_rather_than_an_error() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_bloated_db(&paths);
    std::fs::write(&paths.outcome_path, b"{ this is not json").unwrap();

    let stats = read_stats_at(&paths).expect("a corrupt sidecar must not fail the read");
    assert!(stats.last_compaction.is_none());
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

#[test]
fn freelist_share_is_zero_when_no_free_pages() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    let conn = Connection::open(&paths.database_path).unwrap();
    conn.execute_batch("CREATE TABLE small (id INTEGER PRIMARY KEY);")
        .unwrap();
    conn.execute_batch("INSERT INTO small (id) VALUES (1);")
        .unwrap();
    drop(conn);

    let outcome = compact_before_pool_opens_at(
        &paths,
        CompactionConfig {
            auto_enabled: true,
            auto_max_db_bytes: u64::MAX,
            auto_min_freelist_percent: 1,
        },
    )
    .unwrap();
    assert_eq!(
        outcome,
        CompactionOutcome::Skipped("freelist_below_auto_limit"),
        "no deleted rows means freelist share is 0, below any positive threshold"
    );
}

// Obligation 6(b): verification failure — original untouched, .compacting removed, no backup.
#[test]
fn verification_failure_leaves_original_intact() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_wal_mode_db(&paths);
    let original_bytes = std::fs::read(&paths.database_path).unwrap();
    set_pending_compaction_at(&paths.marker_path, true).unwrap();

    let result = compact_before_pool_opens_at_with_seams(
        &paths,
        config(false),
        &|_path| Err(DatabaseMaintenanceError::Integrity("injected_corrupt".into())),
        None,
    );

    assert!(result.is_err(), "verification failure must propagate as an error");
    let compacting = PathBuf::from(format!("{}.compacting", paths.database_path.display()));
    assert!(
        !compacting.exists(),
        ".compacting scratch must be cleaned up after a verify failure"
    );
    assert!(
        paths.database_path.exists(),
        "original must still be at the live path"
    );
    let after_bytes = std::fs::read(&paths.database_path).unwrap();
    assert_eq!(
        original_bytes, after_bytes,
        "original must be byte-identical to the pre-call snapshot"
    );
    assert!(
        !paths.backup_dir.join("ralphx.db.pre-vacuum").exists(),
        "no backup must exist: nothing destructive happened before verification"
    );
    let record = read_record(&paths.outcome_path).expect("error must be recorded in sidecar");
    assert_eq!(record.outcome, OUTCOME_ERROR);
    let reason = record.reason.as_deref().unwrap_or("");
    assert!(
        reason.starts_with("verify:"),
        "sidecar reason must carry the verify phase prefix, got: {reason}"
    );
}

// Obligation 6(c): rename-in failure — the restore puts the original back at the LIVE path.
//
// The hook deletes the `.compacting` source between steps (e) and (f), so the rename-in
// fails with ENOENT while the live path stays free. That is the only shape in which the
// restore at (f) can actually run, which is the arm this obligation is about.
#[test]
fn rename_in_failure_restores_the_original_to_the_live_path() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_wal_mode_db(&paths);
    let original_bytes = std::fs::read(&paths.database_path).unwrap();
    set_pending_compaction_at(&paths.marker_path, true).unwrap();

    let compacting_path = PathBuf::from(format!("{}.compacting", paths.database_path.display()));
    let database_path = paths.database_path.clone();
    let hook_target = compacting_path.clone();
    let hook = move || {
        assert!(
            !database_path.exists(),
            "the live path must be empty inside the swap window"
        );
        std::fs::remove_file(&hook_target).expect("the compacted replacement must exist here");
    };
    let result = compact_before_pool_opens_at_with_seams(
        &paths,
        config(false),
        &|_path| Ok(()),
        Some(&hook),
    );

    assert!(
        result.is_err(),
        "rename-in failure must propagate as an error"
    );
    assert!(
        paths.database_path.exists(),
        "the restore must put a database back at the live path"
    );
    let after_bytes = std::fs::read(&paths.database_path).unwrap();
    assert_eq!(
        original_bytes, after_bytes,
        "the restored database must be byte-identical to the pre-call original"
    );
    assert!(
        !paths.backup_dir.join("ralphx.db.pre-vacuum").exists(),
        "a successful restore moves the original out of the backup, it does not copy it"
    );

    let record = read_record(&paths.outcome_path).expect("error must be recorded in sidecar");
    assert_eq!(record.outcome, OUTCOME_ERROR);
    let reason = record.reason.as_deref().unwrap_or("");
    assert!(
        reason.starts_with("swap_rename:"),
        "sidecar reason must carry the swap_rename phase prefix, got: {reason}"
    );
    assert!(
        reason.contains(&format!(
            "original restored to {}",
            paths.database_path.display()
        )),
        "the recorded location must be where the database actually is, got: {reason}"
    );
}

// The other arm of the same failure: when the restore cannot run either, the record must
// point at the backup instead of claiming a restore that never happened.
#[test]
fn failed_restore_records_the_backup_as_the_surviving_location() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_wal_mode_db(&paths);
    let original_bytes = std::fs::read(&paths.database_path).unwrap();
    set_pending_compaction_at(&paths.marker_path, true).unwrap();

    let database_path = paths.database_path.clone();
    let hook = move || {
        // A non-empty directory at the now-free live path fails the rename-in and the
        // restore alike (ENOTEMPTY/EISDIR).
        std::fs::create_dir_all(&database_path).ok();
        std::fs::write(database_path.join("blocker"), b"x").ok();
    };
    let result = compact_before_pool_opens_at_with_seams(
        &paths,
        config(false),
        &|_path| Ok(()),
        Some(&hook),
    );

    // Remove the planted directory so TempDir can clean up.
    if paths.database_path.is_dir() {
        std::fs::remove_dir_all(&paths.database_path).ok();
    }

    assert!(
        result.is_err(),
        "rename-in failure must propagate as an error"
    );

    let record = read_record(&paths.outcome_path).expect("error must be recorded in sidecar");
    assert_eq!(record.outcome, OUTCOME_ERROR);
    let reason = record.reason.as_deref().unwrap_or("");
    let backup = paths.backup_dir.join("ralphx.db.pre-vacuum");
    assert!(
        reason.contains("restore failed:")
            && reason.contains(&format!("original preserved at {}", backup.display())),
        "a failed restore must be reported as such, with the backup location, got: {reason}"
    );

    assert!(backup.exists(), "original must survive at the backup path");
    assert_eq!(
        original_bytes,
        std::fs::read(&backup).unwrap(),
        "backup must be byte-identical to the pre-call snapshot"
    );
}

// The rename-out empties the live path until the rename-in lands. Without a breadcrumb,
// dying inside that window leaves the previous run's record — usually `compacted` —
// describing a database that is no longer there.
#[test]
fn swap_window_is_marked_interrupted_until_the_swap_completes() {
    let dir = TempDir::new().unwrap();
    let paths = temp_paths(&dir);
    seed_wal_mode_db(&paths);
    set_pending_compaction_at(&paths.marker_path, true).unwrap();

    // Stand in for the stale record a previous successful run would have left behind.
    write_record(
        &paths.outcome_path,
        &CompactionRecord::compacted(4_096, 512),
    );

    let observed: Mutex<Option<CompactionRecord>> = Mutex::new(None);
    let outcome_path = paths.outcome_path.clone();
    let database_path = paths.database_path.clone();
    let hook = || {
        assert!(
            !database_path.exists(),
            "the live path must be empty inside the swap window"
        );
        *observed.lock().unwrap() = read_record(&outcome_path);
    };
    let outcome = compact_before_pool_opens_at_with_seams(
        &paths,
        config(false),
        &|_path| Ok(()),
        Some(&hook),
    )
    .expect("the swap itself must succeed");

    let in_window = observed
        .lock()
        .unwrap()
        .clone()
        .expect("a record must exist while the live path has no database");
    assert_eq!(in_window.outcome, OUTCOME_ERROR);
    assert_eq!(in_window.reason.as_deref(), Some(REASON_SWAP_INTERRUPTED));

    assert!(matches!(outcome, CompactionOutcome::Compacted { .. }));
    let final_record = read_record(&paths.outcome_path).expect("final record must be written");
    assert_eq!(
        final_record.outcome, OUTCOME_COMPACTED,
        "a completed swap must overwrite the breadcrumb"
    );
}
