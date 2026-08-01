use chrono::{DateTime, Duration, TimeZone, Utc};

use crate::domain::entities::{
    ChatConversationId, ProjectId, RemoteConversationMessageRequest,
    RemoteConversationMessageStatus,
};
use crate::domain::repositories::RemoteConversationMessageRequestRepository;
use crate::infrastructure::sqlite::SqliteRemoteConversationMessageRequestRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_remote_conversation_message_request_repo_tests")
}

fn base_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 1, 13, 0, 0).unwrap()
}

fn pending_request(
    id: &str,
    device_id: &str,
    created_at: DateTime<Utc>,
) -> RemoteConversationMessageRequest {
    RemoteConversationMessageRequest {
        id: id.to_string(),
        conversation_id: ChatConversationId::from_string("11111111-1111-4111-8111-111111111111"),
        project_id: ProjectId::from_string("project-1".to_string()),
        content: format!("content for {id}"),
        provider: "claude".to_string(),
        model_override: Some("opus".to_string()),
        logical_effort: Some("high".to_string()),
        status: RemoteConversationMessageStatus::Pending,
        error_code: None,
        requested_by_device_id: device_id.to_string(),
        agent_run_id: None,
        claimed_at: None,
        created_at,
        updated_at: created_at,
    }
}

#[tokio::test]
async fn create_and_get_round_trip() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationMessageRequestRepository::from_shared(db.shared_conn());
    let expected = pending_request("req-1", "device-a", base_time());

    let created = repo.create_message_request(expected.clone()).await.unwrap();
    assert_eq!(created, expected);

    let fetched = repo.get_message_request("req-1").await.unwrap();
    assert_eq!(fetched, Some(expected));
    assert!(repo.get_message_request("missing").await.unwrap().is_none());
}

#[tokio::test]
async fn claim_flips_exactly_one_pending_per_call() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationMessageRequestRepository::from_shared(db.shared_conn());
    repo.create_message_request(pending_request("req-1", "device-a", base_time()))
        .await
        .unwrap();
    repo.create_message_request(pending_request(
        "req-2",
        "device-a",
        base_time() + Duration::seconds(1),
    ))
    .await
    .unwrap();

    let claim_at = base_time() + Duration::minutes(5);
    let first = repo
        .claim_pending_message_request(claim_at)
        .await
        .unwrap()
        .unwrap();
    let second = repo
        .claim_pending_message_request(claim_at)
        .await
        .unwrap()
        .unwrap();
    let third = repo.claim_pending_message_request(claim_at).await.unwrap();

    assert_eq!(first.status, RemoteConversationMessageStatus::Dispatching);
    assert_eq!(second.status, RemoteConversationMessageStatus::Dispatching);
    assert_eq!(first.claimed_at, Some(claim_at));
    assert_ne!(first.id, second.id);
    // Oldest (created_at ASC) claimed first.
    assert_eq!(first.id, "req-1");
    assert_eq!(second.id, "req-2");
    assert!(third.is_none());
}

#[tokio::test]
async fn complete_sets_dispatched_and_run_id_only_while_dispatching() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationMessageRequestRepository::from_shared(db.shared_conn());
    repo.create_message_request(pending_request("req-1", "device-a", base_time()))
        .await
        .unwrap();
    repo.claim_pending_message_request(base_time() + Duration::minutes(1))
        .await
        .unwrap()
        .unwrap();

    let done_at = base_time() + Duration::minutes(2);
    repo.complete_message_request("req-1", "run-42", done_at)
        .await
        .unwrap();

    let row = repo.get_message_request("req-1").await.unwrap().unwrap();
    assert_eq!(row.status, RemoteConversationMessageStatus::Dispatched);
    assert_eq!(row.agent_run_id.as_deref(), Some("run-42"));
    assert_eq!(row.updated_at, done_at);

    // No-op once no longer dispatching: a late writer cannot overwrite a settled row.
    repo.complete_message_request("req-1", "run-99", base_time() + Duration::minutes(3))
        .await
        .unwrap();
    let row = repo.get_message_request("req-1").await.unwrap().unwrap();
    assert_eq!(row.agent_run_id.as_deref(), Some("run-42"));
}

#[tokio::test]
async fn fail_sets_failed_and_code_only_while_dispatching() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationMessageRequestRepository::from_shared(db.shared_conn());
    repo.create_message_request(pending_request("req-1", "device-a", base_time()))
        .await
        .unwrap();
    repo.claim_pending_message_request(base_time() + Duration::minutes(1))
        .await
        .unwrap()
        .unwrap();

    let fail_at = base_time() + Duration::minutes(2);
    repo.fail_message_request("req-1", "send_failed", fail_at)
        .await
        .unwrap();

    let row = repo.get_message_request("req-1").await.unwrap().unwrap();
    assert_eq!(row.status, RemoteConversationMessageStatus::Failed);
    assert_eq!(row.error_code.as_deref(), Some("send_failed"));

    // Pending row cannot be failed via the dispatching-guarded update.
    repo.create_message_request(pending_request("req-2", "device-a", base_time()))
        .await
        .unwrap();
    repo.fail_message_request("req-2", "should_not_apply", fail_at)
        .await
        .unwrap();
    let row = repo.get_message_request("req-2").await.unwrap().unwrap();
    assert_eq!(row.status, RemoteConversationMessageStatus::Pending);
}

#[tokio::test]
async fn cancel_pending_for_device_only_touches_that_devices_pending() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationMessageRequestRepository::from_shared(db.shared_conn());
    repo.create_message_request(pending_request("req-1", "device-a", base_time()))
        .await
        .unwrap();
    repo.create_message_request(pending_request(
        "req-2",
        "device-a",
        base_time() + Duration::seconds(1),
    ))
    .await
    .unwrap();
    repo.create_message_request(pending_request("req-3", "device-b", base_time()))
        .await
        .unwrap();
    // A dispatching row for device-a must not be cancelled.
    repo.claim_pending_message_request(base_time() + Duration::minutes(1))
        .await
        .unwrap()
        .unwrap();

    let cancelled = repo
        .cancel_pending_message_requests_for_device("device-a", base_time() + Duration::minutes(2))
        .await
        .unwrap();
    assert_eq!(cancelled, 1); // only req-2 remained pending for device-a

    assert_eq!(
        repo.get_message_request("req-1")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteConversationMessageStatus::Dispatching
    );
    assert_eq!(
        repo.get_message_request("req-2")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteConversationMessageStatus::Cancelled
    );
    assert_eq!(
        repo.get_message_request("req-3")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteConversationMessageStatus::Pending
    );
}

#[tokio::test]
async fn fail_stale_only_touches_dispatching_older_than_cutoff() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationMessageRequestRepository::from_shared(db.shared_conn());
    repo.create_message_request(pending_request("old", "device-a", base_time()))
        .await
        .unwrap();
    repo.create_message_request(pending_request(
        "fresh",
        "device-a",
        base_time() + Duration::seconds(1),
    ))
    .await
    .unwrap();

    // Claim "old" first (older created_at), then "fresh", with distinct claim times.
    repo.claim_pending_message_request(base_time() + Duration::minutes(1))
        .await
        .unwrap()
        .unwrap();
    repo.claim_pending_message_request(base_time() + Duration::minutes(10))
        .await
        .unwrap()
        .unwrap();

    // A pending row must never be swept.
    repo.create_message_request(pending_request("still-pending", "device-a", base_time()))
        .await
        .unwrap();

    let cutoff = base_time() + Duration::minutes(5);
    let swept = repo
        .fail_stale_dispatching_message_requests(cutoff, base_time() + Duration::minutes(20))
        .await
        .unwrap();
    assert_eq!(swept, 1);

    assert_eq!(
        repo.get_message_request("old").await.unwrap().unwrap().status,
        RemoteConversationMessageStatus::FailedStale
    );
    assert_eq!(
        repo.get_message_request("fresh")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteConversationMessageStatus::Dispatching
    );
    assert_eq!(
        repo.get_message_request("still-pending")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteConversationMessageStatus::Pending
    );
}
