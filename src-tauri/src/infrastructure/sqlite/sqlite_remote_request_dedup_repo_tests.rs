use super::*;
use crate::testing::SqliteTestDb;

fn repo(db: &SqliteTestDb) -> SqliteRemoteRequestDedupRepository {
    SqliteRemoteRequestDedupRepository::from_db(DbConnection::from_shared(db.shared_conn()))
}

fn device(id: &str) -> RemoteDeviceId {
    RemoteDeviceId(id.to_string())
}

fn record(
    device_id: &str,
    request_id: &str,
    hash: &str,
    expires_at: &str,
) -> RemoteRequestDedupRecord {
    RemoteRequestDedupRecord {
        device_id: device(device_id),
        request_id: request_id.to_string(),
        args_hash: hash.to_string(),
        outcome: RemoteDedupOutcomeKind::Ok,
        response: r#"{"ok":true,"result":null}"#.to_string(),
        created_at: "2026-07-28T10:00:00.000Z".to_string(),
        expires_at: expires_at.to_string(),
    }
}

/// Rule 16: every method goes through `DbConnection::run` / `run_transaction`.
#[test]
fn the_repository_never_locks_the_connection_directly() {
    let source = include_str!("sqlite_remote_request_dedup_repo.rs");
    assert!(!source.contains("conn.lock()"));
    assert!(!source.contains("blocking_lock"));
}

#[tokio::test]
async fn a_successful_read_with_no_row_is_absent_not_an_error() {
    let db = SqliteTestDb::new("remote_dedup_absent");
    let store = repo(&db);

    let found = RemoteRequestDedupRepository::lookup(
        &store,
        &device("device-a"),
        "req-1",
        "2026-07-28T10:00:00.000Z",
    )
    .await
    .expect("a successful read with no row must not be an error");

    assert_eq!(found, RemoteRequestDedupLookup::Absent);
}

#[tokio::test]
async fn a_live_row_is_fresh_and_replays_verbatim() {
    let db = SqliteTestDb::new("remote_dedup_fresh");
    let store = repo(&db);
    let written = record("device-a", "req-1", "hash-a", "2026-07-28T10:10:00.000Z");
    RemoteRequestDedupRepository::record(&store, written.clone())
        .await
        .expect("record should write");

    let found = RemoteRequestDedupRepository::lookup(
        &store,
        &device("device-a"),
        "req-1",
        "2026-07-28T10:05:00.000Z",
    )
    .await
    .expect("lookup should succeed");

    assert_eq!(found, RemoteRequestDedupLookup::Fresh(written));
}

/// The tri-state must distinguish "past TTL" from "never existed": an expired row is a NEW
/// request (execute), while `Absent` is also execute — but only `Expired` proves an id was
/// used before, which the purge path depends on.
#[tokio::test]
async fn a_row_at_or_past_its_ttl_is_expired_not_fresh() {
    let db = SqliteTestDb::new("remote_dedup_expired");
    let store = repo(&db);
    RemoteRequestDedupRepository::record(
        &store,
        record("device-a", "req-1", "hash-a", "2026-07-28T10:10:00.000Z"),
    )
    .await
    .expect("record should write");

    for now in ["2026-07-28T10:10:00.000Z", "2026-07-28T10:11:00.000Z"] {
        let found = RemoteRequestDedupRepository::lookup(&store, &device("device-a"), "req-1", now)
            .await
            .expect("lookup should succeed");
        assert_eq!(
            found,
            RemoteRequestDedupLookup::Expired,
            "TTL boundary is inclusive-expired at {now}"
        );
    }
}

#[tokio::test]
async fn lookup_is_device_scoped_so_one_device_cannot_read_anothers_outcome() {
    let db = SqliteTestDb::new("remote_dedup_scoped");
    let store = repo(&db);
    RemoteRequestDedupRepository::record(
        &store,
        record("device-a", "req-1", "hash-a", "2026-07-28T10:10:00.000Z"),
    )
    .await
    .expect("record should write");

    let found = RemoteRequestDedupRepository::lookup(
        &store,
        &device("device-b"),
        "req-1",
        "2026-07-28T10:05:00.000Z",
    )
    .await
    .expect("lookup should succeed");

    assert_eq!(found, RemoteRequestDedupLookup::Absent);
}

#[tokio::test]
async fn re_recording_the_same_key_is_idempotent() {
    let db = SqliteTestDb::new("remote_dedup_idempotent");
    let store = repo(&db);
    let written = record("device-a", "req-1", "hash-a", "2026-07-28T10:10:00.000Z");
    RemoteRequestDedupRepository::record(&store, written.clone())
        .await
        .expect("first record should write");
    RemoteRequestDedupRepository::record(&store, written.clone())
        .await
        .expect("re-recording the same key must not conflict");

    let found = RemoteRequestDedupRepository::lookup(
        &store,
        &device("device-a"),
        "req-1",
        "2026-07-28T10:05:00.000Z",
    )
    .await
    .expect("lookup should succeed");
    assert_eq!(found, RemoteRequestDedupLookup::Fresh(written));
}

#[tokio::test]
async fn a_command_level_error_outcome_round_trips_as_err() {
    let db = SqliteTestDb::new("remote_dedup_err_outcome");
    let store = repo(&db);
    let mut written = record("device-a", "req-1", "hash-a", "2026-07-28T10:10:00.000Z");
    written.outcome = RemoteDedupOutcomeKind::Err;
    written.response = r#"{"ok":false,"error":"nope"}"#.to_string();
    RemoteRequestDedupRepository::record(&store, written.clone())
        .await
        .expect("record should write");

    let found = RemoteRequestDedupRepository::lookup(
        &store,
        &device("device-a"),
        "req-1",
        "2026-07-28T10:05:00.000Z",
    )
    .await
    .expect("lookup should succeed");
    assert_eq!(found, RemoteRequestDedupLookup::Fresh(written));
}

/// An unrecognised discriminant is a store error, never a silently coerced `Ok` replay.
#[tokio::test]
async fn an_unrecognised_outcome_column_is_a_store_error() {
    let db = SqliteTestDb::new("remote_dedup_bad_outcome");
    let store = repo(&db);
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO remote_request_dedup
                 (device_id, request_id, args_hash, outcome, response, created_at, expires_at)
             VALUES ('device-a', 'req-1', 'hash-a', 'maybe', '{}', 'now', '2999-01-01T00:00:00.000Z')",
            [],
        )
        .expect("malformed row should seed");
    });

    let error = RemoteRequestDedupRepository::lookup(
        &store,
        &device("device-a"),
        "req-1",
        "2026-07-28T10:05:00.000Z",
    )
    .await
    .expect_err("a malformed discriminant must surface as an error");
    assert!(matches!(error, AppError::Database(_)), "got {error:?}");
}

#[tokio::test]
async fn purge_removes_only_expired_rows() {
    let db = SqliteTestDb::new("remote_dedup_purge");
    let store = repo(&db);
    RemoteRequestDedupRepository::record(
        &store,
        record("device-a", "old", "hash-a", "2026-07-28T09:00:00.000Z"),
    )
    .await
    .expect("record should write");
    RemoteRequestDedupRepository::record(
        &store,
        record("device-a", "live", "hash-b", "2026-07-28T11:00:00.000Z"),
    )
    .await
    .expect("record should write");

    let removed = RemoteRequestDedupRepository::purge_expired(&store, "2026-07-28T10:00:00.000Z")
        .await
        .expect("purge should succeed");
    assert_eq!(removed, 1);

    assert_eq!(
        RemoteRequestDedupRepository::lookup(
            &store,
            &device("device-a"),
            "old",
            "2026-07-28T10:00:00.000Z"
        )
        .await
        .expect("lookup should succeed"),
        RemoteRequestDedupLookup::Absent
    );
    assert!(matches!(
        RemoteRequestDedupRepository::lookup(
            &store,
            &device("device-a"),
            "live",
            "2026-07-28T10:00:00.000Z"
        )
        .await
        .expect("lookup should succeed"),
        RemoteRequestDedupLookup::Fresh(_)
    ));
}

fn attachment(id: &str, device_id: &str, size: i64) -> RemoteAttachment {
    RemoteAttachment {
        id: id.to_string(),
        device_id: device(device_id),
        display_name: Some("notes.txt".to_string()),
        mime: "text/plain".to_string(),
        size,
        created_at: "2026-07-28T10:00:00.000Z".to_string(),
    }
}

#[tokio::test]
async fn attachment_reads_are_device_scoped_in_the_query_itself() {
    let db = SqliteTestDb::new("remote_attachment_scoped");
    let store = repo(&db);
    let written = attachment("att-1", "device-a", 10);
    RemoteAttachmentRepository::record(&store, written.clone())
        .await
        .expect("record should write");

    assert_eq!(
        RemoteAttachmentRepository::get_for_device(&store, &device("device-a"), "att-1")
            .await
            .expect("owner read should succeed"),
        Some(written)
    );
    assert_eq!(
        RemoteAttachmentRepository::get_for_device(&store, &device("device-b"), "att-1")
            .await
            .expect("cross-device read should succeed as a query"),
        None,
        "a cross-device read must return nothing, not the row"
    );
}

#[tokio::test]
async fn device_usage_sums_only_that_devices_attachments_and_starts_at_zero() {
    let db = SqliteTestDb::new("remote_attachment_quota");
    let store = repo(&db);

    assert_eq!(
        RemoteAttachmentRepository::device_usage_bytes(&store, &device("device-a"))
            .await
            .expect("empty usage should succeed"),
        0
    );

    for (id, owner, size) in [
        ("att-1", "device-a", 100),
        ("att-2", "device-a", 250),
        ("att-3", "device-b", 9_000),
    ] {
        RemoteAttachmentRepository::record(&store, attachment(id, owner, size))
            .await
            .expect("record should write");
    }

    assert_eq!(
        RemoteAttachmentRepository::device_usage_bytes(&store, &device("device-a"))
            .await
            .expect("usage should succeed"),
        350
    );
    assert_eq!(
        RemoteAttachmentRepository::device_usage_bytes(&store, &device("device-b"))
            .await
            .expect("usage should succeed"),
        9_000
    );
}
