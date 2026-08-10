// Tests for SqliteRemoteEnvironmentRepository (C-1: everything through DbConnection).
// Runs against in-memory SQLite with full migrations.

use ralphx_remote_protocol::Scope;

use crate::domain::entities::remote_environment::{RemoteEnvironmentId, RemoteEnvironmentStatus};
use crate::domain::repositories::{RemoteEnvironmentRepository, UpsertPairedEnvironment};
use crate::infrastructure::sqlite::sqlite_remote_environment_repo::SqliteRemoteEnvironmentRepository;
use crate::testing::SqliteTestDb;

fn setup_repo() -> (SqliteTestDb, SqliteRemoteEnvironmentRepository) {
    let db = SqliteTestDb::new("sqlite-remote-environment-repo");
    let repo = SqliteRemoteEnvironmentRepository::from_shared(db.shared_conn());
    (db, repo)
}

fn corrupt_column(db: &SqliteTestDb, id: &RemoteEnvironmentId, column: &str, value: i64) {
    db.with_connection(|conn| {
        conn.execute_batch("PRAGMA ignore_check_constraints = ON;")
            .expect("test should permit a restored malformed row");
        conn.execute(
            &format!("UPDATE remote_environments SET {column} = ?1 WHERE id = ?2"),
            rusqlite::params![value, id.as_str()],
        )
        .expect("test row should be corrupted");
    });
}

fn pairing(environment_id: &str, url: &str) -> UpsertPairedEnvironment {
    UpsertPairedEnvironment {
        environment_id: environment_id.to_string(),
        name: "Mac Studio".to_string(),
        url: url.to_string(),
        scopes: vec![Scope::UiRead, Scope::UiOperate],
        protocol_version: 1,
    }
}

#[tokio::test]
async fn upsert_inserts_a_pending_add_row_with_client_local_identity() {
    let (_db, repo) = setup_repo();

    let env = repo
        .upsert_paired(pairing("env-1", "https://mac-studio.tailnet.ts.net"))
        .await
        .expect("insert should succeed");

    assert_eq!(env.environment_id, "env-1");
    assert_eq!(env.base_url, "https://mac-studio.tailnet.ts.net");
    assert_eq!(env.candidate_urls, Vec::<String>::new());
    assert_eq!(env.status, RemoteEnvironmentStatus::PendingAdd);
    assert_eq!(
        env.token_secret_ref,
        format!("remote-env:{}:token", env.id.as_str())
    );
    assert_eq!(env.scopes, vec![Scope::UiRead, Scope::UiOperate]);
    assert!(env.last_connected_at.is_none());
}

#[tokio::test]
async fn upsert_dedups_on_environment_id_and_merges_candidate_urls() {
    let (_db, repo) = setup_repo();

    // Pair via MagicDNS, activate, then pair the SAME host via its 100.x address.
    let first = repo
        .upsert_paired(pairing("env-1", "https://mac-studio.tailnet.ts.net"))
        .await
        .expect("first pairing should insert");
    repo.set_status(&first.id, RemoteEnvironmentStatus::Active)
        .await
        .expect("activation should succeed");

    let merged = repo
        .upsert_paired(pairing("env-1", "http://100.101.102.103:3849"))
        .await
        .expect("second pairing should merge");

    // Exactly one row, one environment.
    let all = repo.list().await.expect("list should succeed");
    assert_eq!(all.len(), 1, "same host identity must never yield two rows");

    // The client-local identity and Keychain ref survive the merge, so the token
    // refresh overwrites the same secret instead of orphaning the previous one.
    assert_eq!(merged.id, first.id);
    assert_eq!(merged.token_secret_ref, first.token_secret_ref);
    assert_eq!(merged.base_url, "https://mac-studio.tailnet.ts.net");
    assert_eq!(
        merged.candidate_urls,
        vec!["http://100.101.102.103:3849".to_string()]
    );
    // The re-pair goes back through the staged add machine.
    assert_eq!(merged.status, RemoteEnvironmentStatus::PendingAdd);
}

#[tokio::test]
async fn upsert_does_not_duplicate_known_urls() {
    let (_db, repo) = setup_repo();

    repo.upsert_paired(pairing("env-1", "https://mac-studio.tailnet.ts.net"))
        .await
        .expect("first pairing should insert");
    repo.upsert_paired(pairing("env-1", "http://100.101.102.103:3849"))
        .await
        .expect("second pairing should merge");
    let env = repo
        .upsert_paired(pairing("env-1", "http://100.101.102.103:3849"))
        .await
        .expect("re-pairing a known URL should be a no-op merge");

    assert_eq!(
        env.candidate_urls,
        vec!["http://100.101.102.103:3849".to_string()],
        "an already-known endpoint must not be recorded twice"
    );

    let via_base = repo
        .upsert_paired(pairing("env-1", "https://mac-studio.tailnet.ts.net"))
        .await
        .expect("re-pairing via the base URL should merge");
    assert_eq!(
        via_base.candidate_urls,
        vec!["http://100.101.102.103:3849".to_string()],
        "the base URL must not be duplicated into candidate_urls"
    );
}

#[tokio::test]
async fn unique_environment_id_is_enforced_at_the_schema_level() {
    let (db, repo) = setup_repo();

    let env = repo
        .upsert_paired(pairing("env-1", "https://mac-studio.tailnet.ts.net"))
        .await
        .expect("insert should succeed");

    // Bypass the repo to prove the schema itself refuses a second row.
    let conn = db.new_connection();
    let result = conn.execute(
        "INSERT INTO remote_environments (
            id, environment_id, name, base_url, candidate_urls,
            token_secret_ref, scopes, protocol_version, status
         ) VALUES ('other-id', ?1, 'Clone', 'http://100.1.2.3:3849', '[]',
                   'remote-env:other-id:token', '[]', 1, 'pending_add')",
        [env.environment_id.as_str()],
    );
    assert!(
        result.is_err(),
        "schema must reject a duplicate environment_id"
    );
}

#[tokio::test]
async fn status_round_trips_through_every_reconciler_state() {
    let (_db, repo) = setup_repo();

    let env = repo
        .upsert_paired(pairing("env-1", "https://mac-studio.tailnet.ts.net"))
        .await
        .expect("insert should succeed");

    for status in [
        RemoteEnvironmentStatus::Active,
        RemoteEnvironmentStatus::PendingDelete,
        RemoteEnvironmentStatus::PendingAdd,
    ] {
        repo.set_status(&env.id, status)
            .await
            .expect("status write should succeed");
        let read = repo
            .get(&env.id)
            .await
            .expect("get should succeed")
            .expect("row should exist");
        assert_eq!(read.status, status);
    }
}

#[tokio::test]
async fn set_status_fails_closed_on_a_missing_row() {
    let (_db, repo) = setup_repo();

    let missing = RemoteEnvironmentId::from_string("missing");
    let error = repo
        .set_status(&missing, RemoteEnvironmentStatus::Active)
        .await
        .expect_err("activating a deleted row must not report success");
    assert!(
        matches!(error, crate::error::AppError::NotFound(_)),
        "expected NotFound, got {error:?}"
    );
}

#[tokio::test]
async fn get_by_environment_id_and_delete_round_trip() {
    let (_db, repo) = setup_repo();

    let env = repo
        .upsert_paired(pairing("env-1", "https://mac-studio.tailnet.ts.net"))
        .await
        .expect("insert should succeed");

    let by_identity = repo
        .get_by_environment_id("env-1")
        .await
        .expect("lookup should succeed")
        .expect("row should exist");
    assert_eq!(by_identity.id, env.id);

    repo.delete(&env.id).await.expect("delete should succeed");
    assert!(repo
        .get(&env.id)
        .await
        .expect("get should succeed")
        .is_none());
    // Idempotent: deleting again is a no-op, not an error.
    repo.delete(&env.id)
        .await
        .expect("second delete should be a no-op");
}

#[tokio::test]
async fn touch_last_connected_updates_only_the_timestamp() {
    let (_db, repo) = setup_repo();

    let env = repo
        .upsert_paired(pairing("env-1", "https://mac-studio.tailnet.ts.net"))
        .await
        .expect("insert should succeed");

    repo.touch_last_connected(&env.id, "2026-07-27T19:15:00+00:00")
        .await
        .expect("touch should succeed");

    let read = repo
        .get(&env.id)
        .await
        .expect("get should succeed")
        .expect("row should exist");
    assert_eq!(
        read.last_connected_at.as_deref(),
        Some("2026-07-27T19:15:00+00:00")
    );
    assert_eq!(read.status, RemoteEnvironmentStatus::PendingAdd);
}

#[tokio::test]
async fn malformed_status_and_protocol_version_fail_closed() {
    let (status_db, status_repo) = setup_repo();
    let status_env = status_repo
        .upsert_paired(pairing("env-invalid-status", "https://status.invalid"))
        .await
        .expect("insert should succeed");
    status_db.with_connection(|conn| {
        conn.execute_batch("PRAGMA ignore_check_constraints = ON;")
            .expect("test should permit a restored malformed row");
        conn.execute(
            "UPDATE remote_environments SET status = 'half_paired' WHERE id = ?1",
            [status_env.id.as_str()],
        )
        .expect("status should be corrupted");
    });

    let status_error = status_repo
        .get(&status_env.id)
        .await
        .expect_err("unknown persisted status must be rejected");
    assert!(
        matches!(status_error, crate::error::AppError::Database(message) if
        message.contains("invalid remote environment status"))
    );

    let (protocol_db, protocol_repo) = setup_repo();
    let protocol_env = protocol_repo
        .upsert_paired(pairing("env-invalid-protocol", "https://protocol.invalid"))
        .await
        .expect("insert should succeed");
    corrupt_column(&protocol_db, &protocol_env.id, "protocol_version", -1);

    let protocol_error = protocol_repo
        .get(&protocol_env.id)
        .await
        .expect_err("negative protocol versions must be rejected");
    assert!(
        matches!(protocol_error, crate::error::AppError::Database(message) if
        message.contains("invalid remote environment protocol version"))
    );
}

#[tokio::test]
async fn constructor_and_lookup_error_mapping_are_covered() {
    let db = SqliteTestDb::new("sqlite-remote-environment-new");
    let repo = SqliteRemoteEnvironmentRepository::new(db.new_connection());
    assert!(repo
        .get_by_environment_id("missing")
        .await
        .expect("missing identity lookup should succeed")
        .is_none());

    db.with_connection(|conn| {
        conn.execute("DROP TABLE remote_environments", [])
            .expect("test should remove the repository table");
    });
    let error = repo
        .get_by_environment_id("missing")
        .await
        .expect_err("sqlite lookup failures must be surfaced");
    assert!(matches!(error, crate::error::AppError::Database(message) if
        message.contains("no such table")));
}
