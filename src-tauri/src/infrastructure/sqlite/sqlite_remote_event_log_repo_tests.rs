use super::*;
use crate::testing::SqliteTestDb;

const EPOCH: &str = "epoch-current";
const PRIOR_EPOCH: &str = "epoch-prior";

fn repo(db: &SqliteTestDb) -> SqliteRemoteEventLogRepository {
    SqliteRemoteEventLogRepository::from_db(DbConnection::from_shared(db.shared_conn()))
}

fn seed_settings_row(db: &SqliteTestDb) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT OR IGNORE INTO remote_host_settings
                (id, enabled, exposure_mode, port, environment_id)
             VALUES (1, 0, 'serve', 3849, '8d3d6a07-8e85-4e91-97ce-915fc038fdb2')",
            [],
        )
        .expect("settings row should seed");
    });
}

fn row(seq: u64, epoch: &str, name: &str) -> RemoteEventRow {
    RemoteEventRow {
        seq,
        epoch: epoch.to_string(),
        name: name.to_string(),
        payload: format!(r#"{{"seq":{seq}}}"#),
    }
}

/// C-1 / rule 16: every method goes through `DbConnection::run` / `run_transaction`.
#[test]
fn the_repository_never_locks_the_connection_directly() {
    let source = include_str!("sqlite_remote_event_log_repo.rs");
    assert!(!source.contains("conn.lock()"));
    assert!(!source.contains("blocking_lock"));
}

/// N-M2: the catch-up drain must never take `BEGIN IMMEDIATE`, or a 50k-row replay serializes
/// against the sequencer's own commits and stalls live delivery for every client.
#[test]
fn the_catch_up_drain_uses_a_plain_read_not_a_write_intent_transaction() {
    // Anchor into the IMPL block first: the trait DECLARES `async fn read_range` too (a bodiless
    // signature), and splitting on the whole file lands the scan on that declaration, where no
    // `.run(` call can ever appear — a scan that could never pass proves nothing.
    let source = include_str!("sqlite_remote_event_log_repo.rs");
    let implementation = source
        .split("impl RemoteEventLogStore for SqliteRemoteEventLogRepository")
        .nth(1)
        .expect("the store impl block should exist");
    let drain: String = implementation
        .split("async fn read_range")
        .nth(1)
        .and_then(|rest| rest.split("async fn oldest_seq").next())
        .expect("read_range body should be locatable")
        // Comment-stripped: the body legitimately documents that it is "deliberately NOT
        // run_transaction", and prose about the forbidden call is not the call.
        .lines()
        .map(|line| line.split("//").next().unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        drain.contains(".run("),
        "read_range must go through DbConnection::run"
    );
    assert!(
        !drain.contains("run_transaction"),
        "read_range must not open a write-intent transaction"
    );
}

#[tokio::test]
async fn append_batch_commits_rows_and_the_high_water_together() {
    let db = SqliteTestDb::new("remote-event-log");
    seed_settings_row(&db);
    let repo = repo(&db);

    assert_eq!(repo.high_water().await.unwrap(), 0);
    repo.append_batch(&[
        row(1, EPOCH, "task:created"),
        row(2, EPOCH, "task:status_changed"),
    ])
    .await
    .unwrap();

    assert_eq!(repo.high_water().await.unwrap(), 2);
    let replayed = repo.read_range(EPOCH, 0, 2, 100).await.unwrap();
    assert_eq!(replayed.len(), 2);
    assert_eq!(replayed[0].seq, 1);
    assert_eq!(replayed[1].seq, 2);
    // The raw payload text round-trips byte-for-byte; nothing re-serializes it.
    assert_eq!(replayed[0].payload, r#"{"seq":1}"#);
}

/// A failed commit must leave neither rows nor an advanced high-water: the sequencer resumes
/// from a truthful counter and never publishes a seq that is not durable (§3.4 #2).
#[tokio::test]
async fn a_failed_batch_advances_neither_rows_nor_the_high_water() {
    let db = SqliteTestDb::new("remote-event-log");
    seed_settings_row(&db);
    let repo = repo(&db);
    repo.append_batch(&[row(1, EPOCH, "task:created")])
        .await
        .unwrap();

    // Second row reuses seq 1 — the primary key rejects it and rolls the whole batch back.
    let outcome = repo
        .append_batch(&[row(2, EPOCH, "task:created"), row(1, EPOCH, "task:created")])
        .await;

    assert!(outcome.is_err());
    assert_eq!(repo.high_water().await.unwrap(), 1);
    assert_eq!(repo.read_range(EPOCH, 0, 100, 100).await.unwrap().len(), 1);
}

/// C-14 at the sink, not by precondition: with no settings row there is nowhere to persist the
/// high-water, so committing the log rows anyway would seed the NEXT boot's `next_seq` below seqs
/// that still exist — every insert colliding on the seq primary key, rolling the epoch forever.
/// The batch must fail here instead of wedging a later process.
#[tokio::test]
async fn appending_without_a_settings_row_fails_instead_of_committing_orphaned_seqs() {
    let db = SqliteTestDb::new("remote-event-log");
    // Deliberately NOT seeded: this is the state a host reaches if capture is ever installed
    // before the settings row exists.
    let repo = repo(&db);

    let outcome = repo.append_batch(&[row(1, EPOCH, "task:created")]).await;

    assert!(
        outcome.is_err(),
        "a batch whose high-water cannot be persisted must not commit"
    );
    assert_eq!(repo.read_range(EPOCH, 0, 100, 100).await.unwrap().len(), 0);
    assert_eq!(repo.high_water().await.unwrap(), 0);
}

#[tokio::test]
async fn read_range_is_epoch_scoped_and_bounded_at_both_ends() {
    let db = SqliteTestDb::new("remote-event-log");
    seed_settings_row(&db);
    let repo = repo(&db);
    repo.append_batch(&[
        row(1, PRIOR_EPOCH, "task:created"),
        row(2, EPOCH, "task:created"),
        row(3, EPOCH, "task:created"),
        row(4, EPOCH, "task:created"),
    ])
    .await
    .unwrap();

    let rows = repo.read_range(EPOCH, 2, 3, 100).await.unwrap();
    assert_eq!(
        rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
        vec![3],
        "the drain is exclusive at afterSeq and inclusive at throughSeq"
    );
    assert!(
        repo.read_range(EPOCH, 0, 100, 100)
            .await
            .unwrap()
            .iter()
            .all(|row| row.epoch == EPOCH),
        "prior-epoch rows are unreplayable and must never enter a drain"
    );
}

// ------------------------------------------------------------------------------------------
// P-19: the corrected prune inequality (R4-H3)
// ------------------------------------------------------------------------------------------

#[test]
fn prune_ceiling_never_rises_above_a_live_lease_cursor() {
    // Retention alone would drop everything through 900, and prior epochs through 500 …
    // … but a lease at 100 clamps the ceiling to 100. Rows 101..=900 survive.
    assert_eq!(prune_ceiling(900, 500, 100), 100);
    // The round-3 text had this REVERSED: it would have deleted rows ABOVE the cursor (the
    // hydration delta the barrier exists to protect) and kept the ones nobody needs.
    assert!(prune_ceiling(900, 500, 100) <= 100);
    // No lease at all: retention and the dead-epoch floor decide freely.
    assert_eq!(prune_ceiling(900, 500, u64::MAX), 900);
    // Prior-epoch rows are eligible even when the retention window would keep them …
    assert_eq!(prune_ceiling(0, 500, u64::MAX), 500);
    // … still clamped by the lease floor, which is the single safety boundary.
    assert_eq!(prune_ceiling(0, 500, 300), 300);
}

#[test]
fn retention_ceiling_requires_a_row_to_leave_both_windows() {
    // 10 000 rows, retaining 50 000: nothing is beyond the row cap even if it is ancient.
    assert_eq!(retention_ceiling(10_000, 50_000, 9_000), 0);
    // Beyond the row cap but inside the age window: the age ceiling wins.
    assert_eq!(retention_ceiling(60_000, 50_000, 3_000), 3_000);
    // Beyond both: the row cap is the binding one.
    assert_eq!(retention_ceiling(60_000, 50_000, 55_000), 10_000);
}

#[tokio::test]
async fn prune_deletes_nothing_above_a_live_lease_cursor() {
    let db = SqliteTestDb::new("remote-event-log");
    seed_settings_row(&db);
    let repo = repo(&db);
    let rows = (1..=10)
        .map(|seq| row(seq, EPOCH, "task:created"))
        .collect::<Vec<_>>();
    repo.append_batch(&rows).await.unwrap();

    let outcome = repo
        .prune(RemotePruneRequest {
            current_epoch: EPOCH.to_string(),
            epoch_floor_seq: 0,
            max_seq: 10,
            lease_floor: 4,
            // Retention alone would drop everything.
            retain_rows: 0,
            retain_days: -1,
        })
        .await
        .unwrap();

    assert_eq!(outcome.pruned_floor, 4);
    assert_eq!(outcome.deleted, 4);
    let surviving = repo.read_range(EPOCH, 0, 100, 100).await.unwrap();
    assert_eq!(
        surviving.iter().map(|row| row.seq).collect::<Vec<_>>(),
        vec![5, 6, 7, 8, 9, 10],
        "every row a live cursor still needs (> 4) must survive"
    );
}

#[tokio::test]
async fn prune_with_no_live_lease_still_honours_the_retention_window() {
    let db = SqliteTestDb::new("remote-event-log");
    seed_settings_row(&db);
    let repo = repo(&db);
    let rows = (1..=10)
        .map(|seq| row(seq, EPOCH, "task:created"))
        .collect::<Vec<_>>();
    repo.append_batch(&rows).await.unwrap();

    let outcome = repo
        .prune(RemotePruneRequest {
            current_epoch: EPOCH.to_string(),
            epoch_floor_seq: 0,
            max_seq: 10,
            lease_floor: u64::MAX,
            // Rows are fresh, so the age window protects all of them regardless of the row cap.
            retain_rows: 0,
            retain_days: 7,
        })
        .await
        .unwrap();

    assert_eq!(outcome.deleted, 0);
    assert_eq!(outcome.pruned_floor, 0);
    assert_eq!(repo.read_range(EPOCH, 0, 100, 100).await.unwrap().len(), 10);
}

#[tokio::test]
async fn prior_epoch_rows_are_prune_eligible_immediately_but_never_past_a_lease() {
    let db = SqliteTestDb::new("remote-event-log");
    seed_settings_row(&db);
    let repo = repo(&db);
    repo.append_batch(&[
        row(1, PRIOR_EPOCH, "task:created"),
        row(2, PRIOR_EPOCH, "task:created"),
        row(3, EPOCH, "task:created"),
    ])
    .await
    .unwrap();

    let outcome = repo
        .prune(RemotePruneRequest {
            current_epoch: EPOCH.to_string(),
            epoch_floor_seq: 2,
            max_seq: 3,
            lease_floor: u64::MAX,
            retain_rows: 50_000,
            retain_days: 7,
        })
        .await
        .unwrap();

    assert_eq!(
        outcome.deleted, 2,
        "dead-epoch rows go even inside retention"
    );
    assert_eq!(outcome.pruned_floor, 2);

    // A lease below the epoch floor still wins.
    repo.append_batch(&[row(4, PRIOR_EPOCH, "task:created")])
        .await
        .unwrap();
    let clamped = repo
        .prune(RemotePruneRequest {
            current_epoch: EPOCH.to_string(),
            epoch_floor_seq: 4,
            max_seq: 4,
            lease_floor: 3,
            retain_rows: 50_000,
            retain_days: 7,
        })
        .await
        .unwrap();
    assert_eq!(clamped.pruned_floor, 3);
    assert!(repo
        .read_range(PRIOR_EPOCH, 3, 100, 100)
        .await
        .unwrap()
        .iter()
        .any(|row| row.seq == 4));
}

#[tokio::test]
async fn oldest_seq_reports_the_surviving_floor() {
    let db = SqliteTestDb::new("remote-event-log");
    seed_settings_row(&db);
    let repo = repo(&db);
    assert_eq!(repo.oldest_seq().await.unwrap(), None);

    repo.append_batch(&[row(7, EPOCH, "task:created"), row(8, EPOCH, "task:created")])
        .await
        .unwrap();
    assert_eq!(repo.oldest_seq().await.unwrap(), Some(7));
}

/// P-5(a): the high-water is monotonic. A late batch may not reseed it downward, so a seq is
/// never reused across a restart even if a stale writer arrives out of order.
#[tokio::test]
async fn the_high_water_never_moves_backwards() {
    let db = SqliteTestDb::new("remote-event-log");
    seed_settings_row(&db);
    let repo = repo(&db);

    repo.append_batch(&[row(9, EPOCH, "task:created")])
        .await
        .unwrap();
    assert_eq!(repo.high_water().await.unwrap(), 9);

    repo.append_batch(&[row(3, EPOCH, "task:created")])
        .await
        .unwrap();
    assert_eq!(
        repo.high_water().await.unwrap(),
        9,
        "a lower batch must not reseed the counter downward"
    );
}
