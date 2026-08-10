use chrono::{DateTime, Duration, TimeZone, Utc};

use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversationId, ProjectId,
    RemoteConversationModeSwitchRequest, RemoteConversationModeSwitchStatus,
};
use crate::domain::repositories::RemoteConversationModeSwitchRequestRepository;
use crate::infrastructure::sqlite::SqliteRemoteConversationModeSwitchRequestRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_remote_conversation_mode_switch_request_repo_tests")
}

fn base_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 1, 14, 0, 0).unwrap()
}

const CONVERSATION_A: &str = "11111111-1111-4111-8111-111111111111";
const CONVERSATION_B: &str = "22222222-2222-4222-8222-222222222222";
const PROJECT: &str = "33333333-3333-4333-8333-333333333333";

fn pending_request(
    id: &str,
    device_id: &str,
    conversation_id: &str,
    target_mode: AgentConversationWorkspaceMode,
    created_at: DateTime<Utc>,
) -> RemoteConversationModeSwitchRequest {
    RemoteConversationModeSwitchRequest {
        id: id.to_string(),
        conversation_id: ChatConversationId::from_string(conversation_id),
        project_id: ProjectId::from_string(PROJECT.to_string()),
        target_mode,
        status: RemoteConversationModeSwitchStatus::Pending,
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
    let repo = SqliteRemoteConversationModeSwitchRequestRepository::from_shared(db.shared_conn());
    let expected = pending_request(
        "req-1",
        "device-a",
        CONVERSATION_A,
        AgentConversationWorkspaceMode::Edit,
        base_time(),
    );

    let created = repo
        .create_mode_switch_request(expected.clone())
        .await
        .unwrap();
    assert_eq!(created, expected);

    let fetched = repo.get_mode_switch_request("req-1").await.unwrap();
    assert_eq!(fetched, Some(expected));
    assert!(repo
        .get_mode_switch_request("missing")
        .await
        .unwrap()
        .is_none());
}

/// Every mode must survive the TEXT round trip — a mode that fails to parse back would make the
/// dispatcher unable to claim the row at all.
#[tokio::test]
async fn every_target_mode_round_trips_through_text() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationModeSwitchRequestRepository::from_shared(db.shared_conn());
    for (index, mode) in [
        AgentConversationWorkspaceMode::Chat,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceMode::Plan,
        AgentConversationWorkspaceMode::Tasks,
        AgentConversationWorkspaceMode::Autopilot,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceMode::ReviewPr,
        AgentConversationWorkspaceMode::Automation,
        AgentConversationWorkspaceMode::PersonaBuilder,
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("mode-{index}");
        repo.create_mode_switch_request(pending_request(
            &id,
            "device-a",
            CONVERSATION_A,
            mode,
            base_time(),
        ))
        .await
        .unwrap();
        let fetched = repo.get_mode_switch_request(&id).await.unwrap().unwrap();
        assert_eq!(fetched.target_mode, mode);
    }
}

#[tokio::test]
async fn claim_flips_exactly_one_pending_per_call_oldest_first() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationModeSwitchRequestRepository::from_shared(db.shared_conn());
    repo.create_mode_switch_request(pending_request(
        "req-2",
        "device-a",
        CONVERSATION_B,
        AgentConversationWorkspaceMode::Plan,
        base_time() + Duration::seconds(30),
    ))
    .await
    .unwrap();
    repo.create_mode_switch_request(pending_request(
        "req-1",
        "device-a",
        CONVERSATION_A,
        AgentConversationWorkspaceMode::Edit,
        base_time(),
    ))
    .await
    .unwrap();

    let claim_at = base_time() + Duration::seconds(60);
    let first = repo
        .claim_pending_mode_switch_request(claim_at)
        .await
        .unwrap()
        .expect("a pending row is claimable");
    assert_eq!(first.id, "req-1", "oldest first");
    assert_eq!(first.status, RemoteConversationModeSwitchStatus::Switching);
    assert_eq!(first.claimed_at, Some(claim_at));

    let second = repo
        .claim_pending_mode_switch_request(claim_at)
        .await
        .unwrap()
        .expect("the second pending row is claimable");
    assert_eq!(second.id, "req-2");

    assert!(
        repo.claim_pending_mode_switch_request(claim_at)
            .await
            .unwrap()
            .is_none(),
        "nothing left to claim"
    );
}

/// The CAS guard: terminal writes only apply while the row is `Switching`. A late settle must
/// never resurrect or downgrade an already-settled row.
#[tokio::test]
async fn terminal_writes_are_guarded_on_switching() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationModeSwitchRequestRepository::from_shared(db.shared_conn());
    repo.create_mode_switch_request(pending_request(
        "req-1",
        "device-a",
        CONVERSATION_A,
        AgentConversationWorkspaceMode::Edit,
        base_time(),
    ))
    .await
    .unwrap();

    // Still `Pending` — a completion must NOT apply.
    repo.complete_mode_switch_request("req-1", base_time())
        .await
        .unwrap();
    assert_eq!(
        repo.get_mode_switch_request("req-1")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteConversationModeSwitchStatus::Pending,
        "a completion must not skip the claim"
    );

    repo.claim_pending_mode_switch_request(base_time())
        .await
        .unwrap()
        .unwrap();
    repo.complete_mode_switch_request("req-1", base_time())
        .await
        .unwrap();
    assert_eq!(
        repo.get_mode_switch_request("req-1")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteConversationModeSwitchStatus::Switched
    );

    // Already settled — a later failure must NOT overwrite it.
    repo.fail_mode_switch_request("req-1", "SOME_CODE", base_time())
        .await
        .unwrap();
    let settled = repo
        .get_mode_switch_request("req-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(settled.status, RemoteConversationModeSwitchStatus::Switched);
    assert!(settled.error_code.is_none());
}

#[tokio::test]
async fn already_in_mode_is_a_guarded_benign_terminal_without_an_error_code() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationModeSwitchRequestRepository::from_shared(db.shared_conn());
    repo.create_mode_switch_request(pending_request(
        "req-1",
        "device-a",
        CONVERSATION_A,
        AgentConversationWorkspaceMode::Edit,
        base_time(),
    ))
    .await
    .unwrap();
    repo.claim_pending_mode_switch_request(base_time())
        .await
        .unwrap()
        .unwrap();
    repo.resolve_mode_switch_request_already_in_mode("req-1", base_time())
        .await
        .unwrap();

    let settled = repo
        .get_mode_switch_request("req-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        settled.status,
        RemoteConversationModeSwitchStatus::AlreadyInMode
    );
    assert!(settled.status.is_terminal());
    assert!(settled.error_code.is_none());
}

#[tokio::test]
async fn find_unsettled_scopes_to_the_conversation_and_ignores_terminals() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationModeSwitchRequestRepository::from_shared(db.shared_conn());
    let conversation_a = ChatConversationId::from_string(CONVERSATION_A);
    let conversation_b = ChatConversationId::from_string(CONVERSATION_B);

    repo.create_mode_switch_request(pending_request(
        "req-1",
        "device-a",
        CONVERSATION_A,
        AgentConversationWorkspaceMode::Edit,
        base_time(),
    ))
    .await
    .unwrap();

    assert_eq!(
        repo.find_unsettled_mode_switch_request_for_conversation(&conversation_a)
            .await
            .unwrap()
            .map(|request| request.id),
        Some("req-1".to_string())
    );
    assert!(
        repo.find_unsettled_mode_switch_request_for_conversation(&conversation_b)
            .await
            .unwrap()
            .is_none(),
        "dedupe must not cross conversations"
    );

    // Settle it — the conversation must become requestable again.
    repo.claim_pending_mode_switch_request(base_time())
        .await
        .unwrap()
        .unwrap();
    repo.complete_mode_switch_request("req-1", base_time())
        .await
        .unwrap();
    assert!(
        repo.find_unsettled_mode_switch_request_for_conversation(&conversation_a)
            .await
            .unwrap()
            .is_none(),
        "a settled switch must not lock the conversation forever"
    );
}

#[tokio::test]
async fn cancel_pending_is_scoped_to_the_device_and_skips_claimed_rows() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationModeSwitchRequestRepository::from_shared(db.shared_conn());
    repo.create_mode_switch_request(pending_request(
        "req-1",
        "device-a",
        CONVERSATION_A,
        AgentConversationWorkspaceMode::Edit,
        base_time(),
    ))
    .await
    .unwrap();
    repo.create_mode_switch_request(pending_request(
        "req-2",
        "device-b",
        CONVERSATION_B,
        AgentConversationWorkspaceMode::Plan,
        base_time(),
    ))
    .await
    .unwrap();

    let cancelled = repo
        .cancel_pending_mode_switch_requests_for_device("device-a", base_time())
        .await
        .unwrap();
    assert_eq!(cancelled, 1);
    assert_eq!(
        repo.get_mode_switch_request("req-1")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteConversationModeSwitchStatus::Cancelled
    );
    assert_eq!(
        repo.get_mode_switch_request("req-2")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteConversationModeSwitchStatus::Pending,
        "another device's intent must be untouched"
    );
}

#[tokio::test]
async fn stale_sweep_terminalises_only_switching_rows_claimed_before_the_cutoff() {
    let db = setup_test_db();
    let repo = SqliteRemoteConversationModeSwitchRequestRepository::from_shared(db.shared_conn());
    repo.create_mode_switch_request(pending_request(
        "req-old",
        "device-a",
        CONVERSATION_A,
        AgentConversationWorkspaceMode::Edit,
        base_time(),
    ))
    .await
    .unwrap();
    repo.create_mode_switch_request(pending_request(
        "req-pending",
        "device-a",
        CONVERSATION_B,
        AgentConversationWorkspaceMode::Plan,
        base_time(),
    ))
    .await
    .unwrap();

    // Claim only the first, stamping an old claim time.
    let old_claim = base_time();
    repo.claim_pending_mode_switch_request(old_claim)
        .await
        .unwrap()
        .unwrap();

    let cutoff = base_time() + Duration::seconds(600);
    let swept = repo
        .fail_stale_switching_mode_switch_requests(cutoff, cutoff)
        .await
        .unwrap();
    assert_eq!(swept, 1);
    assert_eq!(
        repo.get_mode_switch_request("req-old")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteConversationModeSwitchStatus::FailedStale
    );
    assert_eq!(
        repo.get_mode_switch_request("req-pending")
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteConversationModeSwitchStatus::Pending,
        "an unclaimed pending row must survive the sweep and stay drainable"
    );
}
