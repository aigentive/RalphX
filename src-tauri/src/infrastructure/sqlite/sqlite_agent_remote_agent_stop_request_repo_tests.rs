use chrono::{DateTime, Duration, TimeZone, Utc};

use crate::domain::entities::{ChatConversationId, RemoteAgentStopRequest, RemoteAgentStopStatus};
use crate::domain::repositories::RemoteAgentStopRequestRepository;
use crate::infrastructure::sqlite::SqliteRemoteAgentStopRequestRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_remote_agent_stop_request_repo_tests")
}

fn base_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 1, 12, 0, 0).unwrap()
}

const CONVERSATION_A: &str = "11111111-1111-4111-8111-111111111111";
const CONVERSATION_B: &str = "22222222-2222-4222-8222-222222222222";

fn pending_request(
    id: &str,
    device_id: &str,
    conversation_id: &str,
    created_at: DateTime<Utc>,
) -> RemoteAgentStopRequest {
    RemoteAgentStopRequest {
        id: id.to_string(),
        conversation_id: ChatConversationId::from_string(conversation_id),
        status: RemoteAgentStopStatus::Pending,
        error_code: None,
        requested_by_device_id: device_id.to_string(),
        claimed_at: None,
        created_at,
        updated_at: created_at,
    }
}

#[tokio::test]
async fn create_and_get_round_trip() {
    let db = setup_test_db();
    let repo = SqliteRemoteAgentStopRequestRepository::from_shared(db.shared_conn());
    let expected = pending_request("req-1", "device-a", CONVERSATION_A, base_time());

    let created = repo.create_stop_request(expected.clone()).await.unwrap();
    assert_eq!(created, expected);

    let fetched = repo.get_stop_request("req-1").await.unwrap();
    assert_eq!(fetched, Some(expected));
    assert!(repo.get_stop_request("missing").await.unwrap().is_none());
}

#[tokio::test]
async fn claim_flips_exactly_one_pending_per_call_oldest_first() {
    let db = setup_test_db();
    let repo = SqliteRemoteAgentStopRequestRepository::from_shared(db.shared_conn());
    repo.create_stop_request(pending_request(
        "req-2",
        "device-a",
        CONVERSATION_B,
        base_time() + Duration::seconds(30),
    ))
    .await
    .unwrap();
    repo.create_stop_request(pending_request(
        "req-1",
        "device-a",
        CONVERSATION_A,
        base_time(),
    ))
    .await
    .unwrap();

    let claim_at = base_time() + Duration::minutes(1);
    let first = repo
        .claim_pending_stop_request(claim_at)
        .await
        .unwrap()
        .expect("a pending row is claimable");
    assert_eq!(first.id, "req-1", "oldest pending is claimed first");
    assert_eq!(first.status, RemoteAgentStopStatus::Stopping);
    assert_eq!(first.claimed_at, Some(claim_at));

    let second = repo
        .claim_pending_stop_request(claim_at)
        .await
        .unwrap()
        .expect("the second pending row is claimable");
    assert_eq!(second.id, "req-2");

    // Nothing pending remains: a third claim must return None rather than re-claim a Stopping row.
    assert!(repo
        .claim_pending_stop_request(claim_at)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn terminal_writes_only_apply_while_stopping() {
    let db = setup_test_db();
    let repo = SqliteRemoteAgentStopRequestRepository::from_shared(db.shared_conn());
    repo.create_stop_request(pending_request(
        "req-1",
        "device-a",
        CONVERSATION_A,
        base_time(),
    ))
    .await
    .unwrap();

    // A completion against a row that was never claimed must not settle it.
    repo.complete_stop_request("req-1", base_time() + Duration::minutes(1))
        .await
        .unwrap();
    assert_eq!(
        repo.get_stop_request("req-1")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteAgentStopStatus::Pending,
        "an unclaimed row must not be completable"
    );

    repo.claim_pending_stop_request(base_time() + Duration::minutes(1))
        .await
        .unwrap()
        .unwrap();
    repo.complete_stop_request("req-1", base_time() + Duration::minutes(2))
        .await
        .unwrap();
    assert_eq!(
        repo.get_stop_request("req-1")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteAgentStopStatus::Stopped
    );

    // A second terminal write cannot downgrade an already-settled row.
    repo.fail_stop_request("req-1", "LATE", base_time() + Duration::minutes(3))
        .await
        .unwrap();
    let stored = repo.get_stop_request("req-1").await.unwrap().unwrap();
    assert_eq!(stored.status, RemoteAgentStopStatus::Stopped);
    assert!(stored.error_code.is_none());
}

#[tokio::test]
async fn no_live_run_is_a_terminal_of_its_own() {
    let db = setup_test_db();
    let repo = SqliteRemoteAgentStopRequestRepository::from_shared(db.shared_conn());
    repo.create_stop_request(pending_request(
        "req-1",
        "device-a",
        CONVERSATION_A,
        base_time(),
    ))
    .await
    .unwrap();
    repo.claim_pending_stop_request(base_time() + Duration::minutes(1))
        .await
        .unwrap()
        .unwrap();

    repo.resolve_stop_request_no_live_run("req-1", base_time() + Duration::minutes(2))
        .await
        .unwrap();

    let stored = repo.get_stop_request("req-1").await.unwrap().unwrap();
    assert_eq!(stored.status, RemoteAgentStopStatus::NoLiveRun);
    assert!(
        stored.error_code.is_none(),
        "NoLiveRun is benign — it must not carry an error code"
    );
    assert!(stored.status.is_terminal());
}

#[tokio::test]
async fn unsettled_lookup_is_conversation_scoped_and_ignores_terminals() {
    let db = setup_test_db();
    let repo = SqliteRemoteAgentStopRequestRepository::from_shared(db.shared_conn());

    assert!(repo
        .find_unsettled_stop_request_for_conversation(&ChatConversationId::from_string(
            CONVERSATION_A
        ))
        .await
        .unwrap()
        .is_none());

    repo.create_stop_request(pending_request(
        "req-a",
        "device-a",
        CONVERSATION_A,
        base_time(),
    ))
    .await
    .unwrap();
    repo.create_stop_request(pending_request(
        "req-b",
        "device-a",
        CONVERSATION_B,
        base_time(),
    ))
    .await
    .unwrap();

    let found = repo
        .find_unsettled_stop_request_for_conversation(&ChatConversationId::from_string(
            CONVERSATION_A,
        ))
        .await
        .unwrap()
        .expect("pending row for conversation A");
    assert_eq!(found.id, "req-a", "the lookup must not cross conversations");

    // A claimed (Stopping) row is still unsettled — a second tap joins it rather than stacking.
    repo.claim_pending_stop_request(base_time() + Duration::minutes(1))
        .await
        .unwrap();
    let found = repo
        .find_unsettled_stop_request_for_conversation(&ChatConversationId::from_string(
            CONVERSATION_A,
        ))
        .await
        .unwrap()
        .expect("a Stopping row is still unsettled");
    assert_eq!(found.status, RemoteAgentStopStatus::Stopping);

    // Once terminal it stops matching, so a later stop can be requested again.
    repo.complete_stop_request("req-a", base_time() + Duration::minutes(2))
        .await
        .unwrap();
    assert!(repo
        .find_unsettled_stop_request_for_conversation(&ChatConversationId::from_string(
            CONVERSATION_A
        ))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn stale_sweep_terminalises_only_old_stopping_claims() {
    let db = setup_test_db();
    let repo = SqliteRemoteAgentStopRequestRepository::from_shared(db.shared_conn());
    repo.create_stop_request(pending_request(
        "req-old",
        "device-a",
        CONVERSATION_A,
        base_time(),
    ))
    .await
    .unwrap();
    repo.claim_pending_stop_request(base_time()).await.unwrap();
    repo.create_stop_request(pending_request(
        "req-fresh",
        "device-a",
        CONVERSATION_B,
        base_time() + Duration::minutes(10),
    ))
    .await
    .unwrap();
    repo.claim_pending_stop_request(base_time() + Duration::minutes(10))
        .await
        .unwrap();

    let swept = repo
        .fail_stale_stopping_stop_requests(
            base_time() + Duration::minutes(5),
            base_time() + Duration::minutes(11),
        )
        .await
        .unwrap();
    assert_eq!(swept, 1);
    assert_eq!(
        repo.get_stop_request("req-old")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteAgentStopStatus::FailedStale
    );
    assert_eq!(
        repo.get_stop_request("req-fresh")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteAgentStopStatus::Stopping,
        "a fresh claim must survive the sweep"
    );
}

#[tokio::test]
async fn device_cancel_only_touches_that_devices_pending_rows() {
    let db = setup_test_db();
    let repo = SqliteRemoteAgentStopRequestRepository::from_shared(db.shared_conn());
    repo.create_stop_request(pending_request(
        "req-a",
        "device-a",
        CONVERSATION_A,
        base_time(),
    ))
    .await
    .unwrap();
    repo.create_stop_request(pending_request(
        "req-b",
        "device-b",
        CONVERSATION_B,
        base_time(),
    ))
    .await
    .unwrap();

    let cancelled = repo
        .cancel_pending_stop_requests_for_device("device-a", base_time() + Duration::minutes(1))
        .await
        .unwrap();
    assert_eq!(cancelled, 1);
    assert_eq!(
        repo.get_stop_request("req-a")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteAgentStopStatus::Cancelled
    );
    assert_eq!(
        repo.get_stop_request("req-b")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteAgentStopStatus::Pending
    );
}
