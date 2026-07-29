use super::*;
use std::fs;
use tempfile::tempdir;

// WAL mode tests require file-based connections — SQLite does not support WAL on :memory:

#[test]
fn test_get_default_db_path_returns_path() {
    let path = get_default_db_path();
    let path_str = path.to_str().unwrap();
    assert!(!path_str.is_empty());
    assert!(path_str.ends_with("src-tauri/ralphx.db"));
}

#[test]
fn test_open_memory_connection_succeeds() {
    let conn = open_memory_connection();
    assert!(conn.is_ok());
}

#[test]
fn test_memory_connection_can_execute_sql() {
    let conn = open_memory_connection().unwrap();
    let result = conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY)", []);
    assert!(result.is_ok());
}

#[test]
fn test_open_connection_creates_file() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    assert!(!db_path.exists());

    let conn = open_connection(&db_path);
    assert!(conn.is_ok());

    // Connection should create the file
    assert!(db_path.exists());
}

#[test]
fn test_open_connection_on_existing_file() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("existing.db");

    // Create file first
    fs::write(&db_path, "").unwrap();
    assert!(db_path.exists());

    let conn = open_connection(&db_path);
    assert!(conn.is_ok());
}

#[test]
fn test_open_connection_error_on_invalid_path() {
    let invalid_path = PathBuf::from("/nonexistent/directory/that/does/not/exist/test.db");
    let result = open_connection(&invalid_path);
    assert!(result.is_err());

    if let Err(AppError::Database(msg)) = result {
        assert!(!msg.is_empty());
    } else {
        panic!("Expected Database error");
    }
}

#[test]
fn test_configure_connection_enables_wal_mode() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("wal_test.db");
    let conn = open_connection(&db_path).unwrap();

    let journal_mode: String = conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();

    assert_eq!(
        journal_mode.to_lowercase(),
        "wal",
        "Expected WAL journal mode, got '{journal_mode}'"
    );
}

#[test]
fn test_configure_connection_sets_busy_timeout() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("busy_timeout_test.db");
    let conn = open_connection(&db_path).unwrap();

    let timeout: i64 = conn
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .unwrap();

    assert_eq!(timeout, 30000, "Expected busy_timeout=30000, got {timeout}");
}

#[test]
fn test_configure_connection_sets_synchronous_normal() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("sync_test.db");
    let conn = open_connection(&db_path).unwrap();

    // synchronous NORMAL = 1
    let synchronous: i64 = conn
        .pragma_query_value(None, "synchronous", |row| row.get(0))
        .unwrap();

    assert_eq!(synchronous, 1, "Expected synchronous=NORMAL (1), got {synchronous}");
}

#[test]
fn test_wal_checkpoint_removes_sidecar_files() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("checkpoint_test.db");

    // Open and write data to trigger WAL file creation
    let conn = open_connection(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE items (id INTEGER PRIMARY KEY, val TEXT);
         INSERT INTO items VALUES (1, 'hello');",
    )
    .unwrap();

    // WAL sidecar may or may not exist yet depending on SQLite internals.
    // Run checkpoint to fold WAL back into main DB.
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();

    let wal_path = dir.path().join("checkpoint_test.db-wal");
    let shm_path = dir.path().join("checkpoint_test.db-shm");

    // After TRUNCATE checkpoint with no other readers/writers, sidecar files
    // should either not exist or be empty (SQLite may leave 0-byte files).
    if wal_path.exists() {
        let wal_size = fs::metadata(&wal_path).unwrap().len();
        assert_eq!(
            wal_size, 0,
            "WAL file should be empty after TRUNCATE checkpoint, got {wal_size} bytes"
        );
    }
    // shm file presence is filesystem-dependent; just verify DB data is intact
    drop(shm_path);

    // Verify data is accessible after checkpoint
    let count: i64 = conn
        .query_row("SELECT count(*) FROM items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "Data should be intact after WAL checkpoint");
}
