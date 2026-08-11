use super::*;
use crate::infrastructure::sqlite::connection::open_connection;
use tempfile::tempdir;
use tokio::sync::Mutex;

#[test]
fn db_lock_slow_threshold_covers_wait_and_hold_directions() {
    let thresholds = DbLockTelemetryThresholds {
        wait_warn_ms: 100,
        hold_warn_ms: 250,
    };

    assert!(!db_lock_observation_is_slow(99, 249, thresholds));
    assert!(db_lock_observation_is_slow(100, 1, thresholds));
    assert!(db_lock_observation_is_slow(1, 250, thresholds));
}

#[test]
fn db_caller_metadata_uses_only_the_file_stem() {
    assert_eq!(
        db_caller_module("/Users/example/private/project/sqlite_task_repo.rs"),
        "sqlite_task_repo"
    );
}

#[tokio::test]
async fn test_run_transaction_commits_on_success() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("tx_commit.db");
    let conn = open_connection(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, val TEXT)")
        .unwrap();
    let db = DbConnection::new(conn);

    db.run_transaction(|conn| {
        conn.execute("INSERT INTO items VALUES (1, 'hello')", [])?;
        Ok(())
    })
    .await
    .unwrap();

    // Verify data persisted
    let count: i64 = db
        .run(|conn| Ok(conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0))?))
        .await
        .unwrap();

    assert_eq!(count, 1, "Transaction should have committed the row");
}

#[tokio::test]
async fn test_run_transaction_rolls_back_on_error() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("tx_rollback.db");
    let conn = open_connection(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, val TEXT)")
        .unwrap();
    let db = DbConnection::new(conn);

    // First insert one row successfully
    db.run_transaction(|conn| {
        conn.execute("INSERT INTO items VALUES (1, 'first')", [])?;
        Ok(())
    })
    .await
    .unwrap();

    // Now run a transaction that fails mid-way
    let result: crate::error::AppResult<()> = db
        .run_transaction(|conn| {
            conn.execute("INSERT INTO items VALUES (2, 'second')", [])?;
            // Simulate error — returns Err to trigger rollback
            Err(crate::error::AppError::Database(
                "simulated failure".to_string(),
            ))
        })
        .await;

    assert!(result.is_err(), "Transaction should have returned an error");

    // Row 2 must not exist — rollback was applied
    let count: i64 = db
        .run(|conn| Ok(conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0))?))
        .await
        .unwrap();

    assert_eq!(
        count, 1,
        "Rolled-back row should not be present; only the first row remains"
    );
}

#[tokio::test]
async fn test_run_transaction_returns_value_from_closure() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("tx_return.db");
    let conn = open_connection(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, val TEXT)")
        .unwrap();
    let db = DbConnection::new(conn);

    let inserted_id: i64 = db
        .run_transaction(|conn| {
            conn.execute("INSERT INTO items VALUES (42, 'answer')", [])?;
            Ok(conn.last_insert_rowid())
        })
        .await
        .unwrap();

    assert_eq!(
        inserted_id, 42,
        "Should return last_insert_rowid from the transaction"
    );
}

#[tokio::test]
async fn test_run_transaction_multiple_operations_atomic() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("tx_atomic.db");
    let conn = open_connection(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE a (id INTEGER PRIMARY KEY);
         CREATE TABLE b (id INTEGER PRIMARY KEY);",
    )
    .unwrap();
    let db = DbConnection::new(conn);

    // Successful transaction touching two tables
    db.run_transaction(|conn| {
        conn.execute("INSERT INTO a VALUES (1)", [])?;
        conn.execute("INSERT INTO b VALUES (1)", [])?;
        Ok(())
    })
    .await
    .unwrap();

    let (count_a, count_b): (i64, i64) = db
        .run(|conn| {
            let a = conn.query_row("SELECT count(*) FROM a", [], |r| r.get(0))?;
            let b = conn.query_row("SELECT count(*) FROM b", [], |r| r.get(0))?;
            Ok((a, b))
        })
        .await
        .unwrap();

    assert_eq!(count_a, 1);
    assert_eq!(count_b, 1);

    // Failed transaction — inserts rolled back atomically
    let _err: crate::error::AppResult<()> = db
        .run_transaction(|conn| {
            conn.execute("INSERT INTO a VALUES (2)", [])?;
            conn.execute("INSERT INTO b VALUES (2)", [])?;
            Err(crate::error::AppError::Database("abort".to_string()))
        })
        .await;

    let (count_a2, count_b2): (i64, i64) = db
        .run(|conn| {
            let a = conn.query_row("SELECT count(*) FROM a", [], |r| r.get(0))?;
            let b = conn.query_row("SELECT count(*) FROM b", [], |r| r.get(0))?;
            Ok((a, b))
        })
        .await
        .unwrap();

    assert_eq!(
        count_a2, 1,
        "Rolled-back insert in table a should not persist"
    );
    assert_eq!(
        count_b2, 1,
        "Rolled-back insert in table b should not persist"
    );
}

// ============================================================================
// WAL concurrent read/write tests (step 8)
// ============================================================================
// WAL mode is set by open_connection() via PRAGMA journal_mode=WAL.
// These tests verify cross-connection behaviour: two DbConnections pointing to
// the same file can interleave reads and writes without SQLITE_BUSY errors.
// NOTE: WAL is not supported on :memory: databases, so these use tempfile DBs.
// ============================================================================

/// Two separate DbConnections to the same WAL-mode file can read and write
/// concurrently without "database is locked" errors.
#[tokio::test]
async fn test_wal_two_connections_cross_conn_read_write() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("wal_cross.db");

    // conn1: writer
    let conn1 = open_connection(&db_path).unwrap();
    conn1
        .execute_batch("CREATE TABLE counters (id INTEGER PRIMARY KEY, n INTEGER NOT NULL)")
        .unwrap();
    conn1
        .execute("INSERT INTO counters VALUES (1, 0)", [])
        .unwrap();
    let db_writer = DbConnection::new(conn1);

    // conn2: reader — separate physical connection to the same WAL-mode file
    let conn2 = open_connection(&db_path).unwrap();
    let db_reader = DbConnection::new(conn2);

    // Writer increments the counter 20 times
    for i in 1i64..=20 {
        db_writer
            .run_transaction(move |conn| {
                conn.execute(
                    "UPDATE counters SET n = ?1 WHERE id = 1",
                    rusqlite::params![i],
                )?;
                Ok(())
            })
            .await
            .expect("Write transaction should not fail");
    }

    // Reader must see the committed value — should not receive SQLITE_BUSY in WAL mode
    let final_n: i64 = db_reader
        .run(|conn| Ok(conn.query_row("SELECT n FROM counters WHERE id = 1", [], |r| r.get(0))?))
        .await
        .expect("Cross-connection read in WAL mode should not fail with SQLITE_BUSY");

    assert_eq!(final_n, 20, "Reader must see the committed counter value");
}

/// Cross-connection writes both reach the database (WAL serialises writers).
/// The second writer's update is visible from a third reader connection.
#[tokio::test]
async fn test_wal_two_writers_serialised() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("wal_two_writers.db");

    let conn1 = open_connection(&db_path).unwrap();
    conn1
        .execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, src TEXT NOT NULL)")
        .unwrap();
    let db1 = DbConnection::new(conn1);
    let db2 = DbConnection::new(open_connection(&db_path).unwrap());
    let db3 = DbConnection::new(open_connection(&db_path).unwrap());

    // db1 inserts row 1
    db1.run_transaction(|conn| {
        conn.execute("INSERT INTO items VALUES (1, 'db1')", [])?;
        Ok(())
    })
    .await
    .expect("db1 write should succeed");

    // db2 inserts row 2 (separate writer connection, serialised by WAL)
    db2.run_transaction(|conn| {
        conn.execute("INSERT INTO items VALUES (2, 'db2')", [])?;
        Ok(())
    })
    .await
    .expect("db2 write should succeed in WAL mode");

    // db3 reads both rows — must see commits from both db1 and db2
    let count: i64 = db3
        .run(|conn| Ok(conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0))?))
        .await
        .expect("db3 read should not fail");

    assert_eq!(
        count, 2,
        "Both writes must be visible to a third connection"
    );
}

/// Read-then-write transactions on pooled WAL connections must not fail with
/// `database is locked` when another writer commits in between the read and write.
#[tokio::test]
async fn test_pooled_transactions_reserve_writer_lock_before_reads() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("wal_upgrade_safe.db");
    let conn = open_connection(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, val TEXT NOT NULL)")
        .unwrap();

    let shared = Arc::new(Mutex::new(conn));
    let db1 = DbConnection::from_shared(Arc::clone(&shared));
    let db2 = DbConnection::from_shared(Arc::clone(&shared));

    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();

    let tx1 = tokio::spawn(async move {
        db1.run_transaction(move |conn| {
            let _: i64 = conn.query_row("SELECT count(*) FROM items", [], |row| row.get(0))?;
            let _ = started_tx.send(());
            std::thread::sleep(std::time::Duration::from_millis(150));
            conn.execute("INSERT INTO items VALUES (1, 'tx1')", [])?;
            Ok(())
        })
        .await
    });

    let tx2 = tokio::spawn(async move {
        let _ = started_rx.await;
        db2.run_transaction(|conn| {
            conn.execute("INSERT INTO items VALUES (2, 'tx2')", [])?;
            Ok(())
        })
        .await
    });

    tx1.await
        .unwrap()
        .expect("first transaction should succeed");
    tx2.await
        .unwrap()
        .expect("second transaction should succeed");

    let db_verify = DbConnection::new(open_connection(&db_path).unwrap());
    let count: i64 = db_verify
        .run(|conn| Ok(conn.query_row("SELECT count(*) FROM items", [], |row| row.get(0))?))
        .await
        .unwrap();

    assert_eq!(
        count, 2,
        "Both pooled transactions must commit successfully"
    );
}

#[tokio::test]
async fn test_from_shared_file_backed_reuses_pooled_backend() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("pooled_shared.db");
    let conn = open_connection(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, val TEXT)")
        .unwrap();

    let shared = Arc::new(Mutex::new(conn));
    let db1 = DbConnection::from_shared(Arc::clone(&shared));
    let db2 = DbConnection::from_shared(Arc::clone(&shared));

    assert!(
        matches!(db1.backend.as_ref(), DbBackend::Pool(_)),
        "File-backed shared connections should use the pooled backend"
    );
    assert!(
        Arc::ptr_eq(&db1.backend, &db2.backend),
        "Shared file-backed connections should reuse the cached pooled backend"
    );
}

#[tokio::test]
async fn test_pooled_backend_prefers_unlocked_connection() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("pooled_available.db");
    let conn = open_connection(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY)")
        .unwrap();

    let shared = Arc::new(Mutex::new(conn));
    let db = DbConnection::from_shared(Arc::clone(&shared));

    let DbBackend::Pool(pool) = db.backend.as_ref() else {
        panic!("expected file-backed pooled backend");
    };

    let _locked_first_lane = pool.connections[0].try_lock().expect("lock lane 0");
    pool.next_index.store(0, Ordering::Relaxed);

    let (_conn, backend, idx, pick) = db.pick_connection();

    assert_eq!(backend, "pool");
    assert_ne!(idx, 0, "pool should skip the locked first lane");
    assert_eq!(pick, "first_available");
}

#[tokio::test]
async fn test_from_shared_in_memory_uses_single_backend() {
    let shared = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
    let db = DbConnection::from_shared(Arc::clone(&shared));

    assert!(
        matches!(db.backend.as_ref(), DbBackend::Single(_)),
        "In-memory shared connections should stay on the isolated single-connection path"
    );
}
