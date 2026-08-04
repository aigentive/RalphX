use chrono::{Duration, Utc};

use crate::domain::entities::{
    AgentRunId, ChatConversationId, DelegationPark, DelegationParkId, DelegationParkJob,
    DelegationParkState, DelegationWakePolicy,
};
use crate::domain::repositories::DelegationParkRepository;
use crate::infrastructure::memory::MemoryDelegationParkRepo;
use crate::infrastructure::sqlite::SqliteDelegationParkRepo;
use crate::testing::SqliteTestDb;

fn setup() -> (SqliteTestDb, SqliteDelegationParkRepo) {
    let db = SqliteTestDb::new("sqlite_delegation_park_repo_tests");
    let repo = SqliteDelegationParkRepo::from_shared(db.shared_conn());
    (db, repo)
}

fn park(
    conversation_id: ChatConversationId,
    generation: i64,
    deadline_at: chrono::DateTime<Utc>,
) -> DelegationPark {
    let now = Utc::now();
    DelegationPark {
        id: DelegationParkId::new(),
        parent_conversation_id: conversation_id,
        parent_agent_run_id: AgentRunId::new(),
        generation,
        wake_policy: DelegationWakePolicy::AllSettled,
        wake_on_failure: true,
        state: DelegationParkState::Armed,
        deadline_at,
        wake_claimed_at: None,
        wake_attempts: 0,
        last_error: None,
        created_at: now,
        updated_at: now,
        jobs: vec![DelegationParkJob {
            job_id: "job-1".to_string(),
            delegated_session_id: "session-1".to_string(),
            delegated_agent_run_id: AgentRunId::new(),
            settled_status: None,
        }],
    }
}

#[tokio::test]
async fn arm_and_get_round_trip_jobs() {
    let (_db, repo) = setup();
    let park = park(
        ChatConversationId::new(),
        4,
        Utc::now() + Duration::hours(1),
    );

    repo.arm(park.clone()).await.unwrap();

    let loaded = repo.get(&park.id).await.unwrap().unwrap();
    assert_eq!(loaded.id, park.id);
    assert_eq!(loaded.parent_conversation_id, park.parent_conversation_id);
    assert_eq!(loaded.jobs, park.jobs);
}

#[tokio::test]
async fn gets_armed_park_for_conversation_and_misses_other_conversations() {
    let (_db, repo) = setup();
    let park = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    repo.arm(park.clone()).await.unwrap();

    assert_eq!(
        repo.get_armed_for_conversation(&park.parent_conversation_id)
            .await
            .unwrap()
            .unwrap()
            .id,
        park.id
    );
    assert!(repo
        .get_armed_for_conversation(&ChatConversationId::new())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn records_a_job_settlement_and_updates_the_park() {
    let (_db, repo) = setup();
    let park = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    let delegated_run_id = park.jobs[0].delegated_agent_run_id;
    repo.arm(park.clone()).await.unwrap();

    repo.record_job_settled(&park.id, &delegated_run_id, "completed")
        .await
        .unwrap();

    assert_eq!(
        repo.get(&park.id).await.unwrap().unwrap().jobs[0].settled_status,
        Some("completed".to_string())
    );
}

#[tokio::test]
async fn claim_wake_only_succeeds_once_and_checks_generation() {
    let (_db, repo) = setup();
    let park = park(
        ChatConversationId::new(),
        7,
        Utc::now() + Duration::hours(1),
    );
    repo.arm(park.clone()).await.unwrap();

    assert!(!repo.claim_wake(&park.id, 6).await.unwrap());
    assert!(repo.claim_wake(&park.id, 7).await.unwrap());
    assert!(!repo.claim_wake(&park.id, 7).await.unwrap());
    assert_eq!(
        repo.get(&park.id).await.unwrap().unwrap().state,
        DelegationParkState::Waking
    );
}

#[tokio::test]
async fn wake_claim_marker_is_stamped_preserved_on_settle_and_cleared_on_reset() {
    let (_db, repo) = setup();
    let settled_park = park(
        ChatConversationId::new(),
        7,
        Utc::now() + Duration::hours(1),
    );
    repo.arm(settled_park.clone()).await.unwrap();

    let before_claim = Utc::now();
    assert!(repo
        .claim_wake(&settled_park.id, settled_park.generation)
        .await
        .unwrap());
    let claimed = repo.get(&settled_park.id).await.unwrap().unwrap();
    assert!(claimed.wake_claimed_at.is_some_and(|at| at >= before_claim));

    repo.settle(&settled_park.id, DelegationParkState::Woken, None)
        .await
        .unwrap();
    let settled = repo.get(&settled_park.id).await.unwrap().unwrap();
    assert_eq!(settled.wake_claimed_at, claimed.wake_claimed_at);

    let reset_park = park(
        ChatConversationId::new(),
        8,
        Utc::now() + Duration::hours(1),
    );
    repo.arm(reset_park.clone()).await.unwrap();
    assert!(repo
        .claim_wake(&reset_park.id, reset_park.generation)
        .await
        .unwrap());
    assert!(repo
        .get(&reset_park.id)
        .await
        .unwrap()
        .unwrap()
        .wake_claimed_at
        .is_some());
    assert!(repo.reset_wake_claim(&reset_park.id).await.unwrap());
    assert!(repo
        .get(&reset_park.id)
        .await
        .unwrap()
        .unwrap()
        .wake_claimed_at
        .is_none());
}

#[tokio::test]
async fn concurrent_claim_wake_has_exactly_one_winner() {
    let (_db, repo) = setup();
    let park = park(
        ChatConversationId::new(),
        3,
        Utc::now() + Duration::hours(1),
    );
    repo.arm(park.clone()).await.unwrap();

    let (first, second) = tokio::join!(repo.claim_wake(&park.id, 3), repo.claim_wake(&park.id, 3));

    assert_ne!(first.unwrap(), second.unwrap());
}

#[tokio::test]
async fn record_wake_failure_increments_and_returns_the_durable_count() {
    let (_db, repo) = setup();
    let park = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    repo.arm(park.clone()).await.unwrap();

    assert_eq!(
        repo.record_wake_failure(&park.id, "first").await.unwrap(),
        1
    );
    assert_eq!(
        repo.record_wake_failure(&park.id, "second").await.unwrap(),
        2
    );

    let loaded = repo.get(&park.id).await.unwrap().unwrap();
    assert_eq!(loaded.wake_attempts, 2);
    assert_eq!(loaded.last_error.as_deref(), Some("second"));
}

#[tokio::test]
async fn memory_record_wake_failure_increments_and_returns_the_durable_count() {
    let repo = MemoryDelegationParkRepo::new();
    let park = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    repo.arm(park.clone()).await.unwrap();

    assert_eq!(
        repo.record_wake_failure(&park.id, "first").await.unwrap(),
        1
    );
    assert_eq!(
        repo.record_wake_failure(&park.id, "second").await.unwrap(),
        2
    );

    let loaded = repo.get(&park.id).await.unwrap().unwrap();
    assert_eq!(loaded.wake_attempts, 2);
    assert_eq!(loaded.last_error.as_deref(), Some("second"));
}

#[tokio::test]
async fn lists_only_waking_parks_stalled_before_the_cutoff() {
    let (_db, repo) = setup();
    let stale = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    let fresh = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    let armed = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    let terminal = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    for value in [&stale, &fresh, &armed, &terminal] {
        repo.arm((*value).clone()).await.unwrap();
    }
    assert!(repo.claim_wake(&stale.id, stale.generation).await.unwrap());
    assert!(repo.claim_wake(&fresh.id, fresh.generation).await.unwrap());
    assert!(repo
        .claim_wake(&terminal.id, terminal.generation)
        .await
        .unwrap());
    repo.settle(&terminal.id, DelegationParkState::Woken, None)
        .await
        .unwrap();

    let cutoff = Utc::now() - Duration::minutes(1);
    let stale_at = cutoff - Duration::minutes(1);
    _db.with_connection(|conn| {
        for id in [&stale.id, &armed.id, &terminal.id] {
            conn.execute(
                "UPDATE delegation_parks SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![stale_at.to_rfc3339(), id.as_str()],
            )
            .unwrap();
        }
    });

    let parks = repo.list_wake_stalled(cutoff).await.unwrap();
    assert_eq!(parks.len(), 1);
    assert_eq!(parks[0].id, stale.id);
}

#[tokio::test]
async fn reset_wake_claim_transitions_waking_once_and_skips_armed_parks() {
    let (_db, repo) = setup();
    let waking = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    let armed = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    repo.arm(waking.clone()).await.unwrap();
    repo.arm(armed.clone()).await.unwrap();
    assert!(repo
        .claim_wake(&waking.id, waking.generation)
        .await
        .unwrap());

    assert!(repo.reset_wake_claim(&waking.id).await.unwrap());
    assert!(!repo.reset_wake_claim(&waking.id).await.unwrap());
    assert!(!repo.reset_wake_claim(&armed.id).await.unwrap());
    assert_eq!(
        repo.get(&waking.id).await.unwrap().unwrap().state,
        DelegationParkState::Armed
    );
}

#[tokio::test]
async fn reset_wake_claim_clears_the_abandoned_dispatchers_attempts() {
    let (_db, repo) = setup();
    let park = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    repo.arm(park.clone()).await.unwrap();
    assert!(repo.claim_wake(&park.id, park.generation).await.unwrap());
    assert_eq!(
        repo.record_wake_failure(&park.id, "enqueue failed")
            .await
            .unwrap(),
        1
    );

    assert!(repo.reset_wake_claim(&park.id).await.unwrap());

    // The recovering dispatcher must not inherit the crashed one's spent budget; `park_max_secs`
    // still bounds total effort.
    let reclaimed = repo.get(&park.id).await.unwrap().unwrap();
    assert_eq!(reclaimed.wake_attempts, 0);
    assert_eq!(reclaimed.last_error.as_deref(), Some("enqueue failed"));
    assert_eq!(
        repo.record_wake_failure(&park.id, "enqueue failed again")
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn settle_writes_terminal_state_and_error() {
    let (_db, repo) = setup();
    let park = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    repo.arm(park.clone()).await.unwrap();

    repo.settle(
        &park.id,
        DelegationParkState::Failed,
        Some("dispatch failed"),
    )
    .await
    .unwrap();

    let loaded = repo.get(&park.id).await.unwrap().unwrap();
    assert_eq!(loaded.state, DelegationParkState::Failed);
    assert_eq!(loaded.last_error.as_deref(), Some("dispatch failed"));
}

#[tokio::test]
async fn disarm_armed_for_parent_run_is_a_compare_and_swap_and_round_trips_storage() {
    let (_db, repo) = setup();
    let conversation_id = ChatConversationId::new();
    let armed = park(conversation_id.clone(), 1, Utc::now() + Duration::hours(1));
    let mut waking = park(conversation_id.clone(), 2, Utc::now() + Duration::hours(1));
    waking.parent_agent_run_id = armed.parent_agent_run_id;
    let unrelated = park(conversation_id.clone(), 3, Utc::now() + Duration::hours(1));
    repo.arm(armed.clone()).await.unwrap();
    repo.arm(waking.clone()).await.unwrap();
    repo.arm(unrelated.clone()).await.unwrap();
    assert!(repo
        .claim_wake(&waking.id, waking.generation)
        .await
        .unwrap());

    assert_eq!(
        repo.disarm_armed_for_parent_run(&conversation_id, &armed.parent_agent_run_id)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        repo.disarm_armed_for_parent_run(&conversation_id, &armed.parent_agent_run_id)
            .await
            .unwrap(),
        0,
        "the same terminal path must not overwrite an already-settled park"
    );
    assert_eq!(
        repo.get(&armed.id).await.unwrap().unwrap().state,
        DelegationParkState::Disarmed
    );
    assert_eq!(
        repo.get(&waking.id).await.unwrap().unwrap().state,
        DelegationParkState::Waking,
        "an already-claimed wake retains its own settlement authority"
    );
    assert_eq!(
        repo.get(&unrelated.id).await.unwrap().unwrap().state,
        DelegationParkState::Armed,
        "a different parent run must remain armed"
    );
}

#[tokio::test]
async fn supersede_for_conversation_only_counts_armed_parks() {
    let (_db, repo) = setup();
    let conversation_id = ChatConversationId::new();
    let first = park(conversation_id, 1, Utc::now() + Duration::hours(1));
    let second = park(conversation_id, 2, Utc::now() + Duration::hours(1));
    let other = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    for value in [&first, &second, &other] {
        repo.arm((*value).clone()).await.unwrap();
    }

    assert_eq!(
        repo.supersede_for_conversation(&conversation_id)
            .await
            .unwrap(),
        2
    );
    assert_eq!(repo.list_armed().await.unwrap().len(), 1);
}

#[tokio::test]
async fn lists_only_armed_parks_past_their_deadline() {
    let (_db, repo) = setup();
    let expired = park(
        ChatConversationId::new(),
        1,
        Utc::now() - Duration::minutes(1),
    );
    let pending = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::minutes(1),
    );
    repo.arm(expired.clone()).await.unwrap();
    repo.arm(pending).await.unwrap();

    let parks = repo.list_expired(Utc::now()).await.unwrap();
    assert_eq!(parks.len(), 1);
    assert_eq!(parks[0].id, expired.id);
}

#[tokio::test]
async fn enum_columns_round_trip_as_text_and_reject_invalid_values() {
    let (_db, repo) = setup();
    let mut park = park(
        ChatConversationId::new(),
        1,
        Utc::now() + Duration::hours(1),
    );
    park.wake_policy = DelegationWakePolicy::AnySettled;
    repo.arm(park.clone()).await.unwrap();

    _db.with_connection(|conn| {
        let values: (String, String) = conn
            .query_row(
                "SELECT wake_policy, state FROM delegation_parks WHERE id = ?1",
                [park.id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(values, ("any_settled".to_string(), "armed".to_string()));
        conn.execute(
            "UPDATE delegation_parks SET state = 'not_a_real_state' WHERE id = ?1",
            [park.id.as_str()],
        )
        .unwrap();
    });

    assert!(repo.get(&park.id).await.is_err());
}

#[tokio::test]
async fn memory_repository_enforces_identity_and_orders_active_queries() {
    let repo = MemoryDelegationParkRepo::default();
    let conversation_id = ChatConversationId::new();
    let delegated_run_id = AgentRunId::new();
    let now = Utc::now();
    let mut late = park(conversation_id.clone(), 2, now + Duration::hours(2));
    late.created_at = now + Duration::seconds(2);
    late.updated_at = late.created_at;
    late.jobs[0].delegated_agent_run_id = delegated_run_id;
    let mut early = park(conversation_id.clone(), 1, now + Duration::hours(1));
    early.created_at = now + Duration::seconds(1);
    early.updated_at = early.created_at;
    early.jobs[0].delegated_agent_run_id = delegated_run_id;

    repo.arm(late.clone()).await.unwrap();
    repo.arm(early.clone()).await.unwrap();
    assert!(matches!(
        repo.arm(early.clone()).await.unwrap_err(),
        crate::error::AppError::Database(_)
    ));
    assert!(repo.get(&DelegationParkId::new()).await.unwrap().is_none());
    assert_eq!(repo.get(&early.id).await.unwrap().unwrap().jobs, early.jobs);
    assert_eq!(
        repo.get_armed_for_conversation(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .id,
        early.id
    );
    assert!(repo
        .get_armed_for_conversation(&ChatConversationId::new())
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        repo.list_armed()
            .await
            .unwrap()
            .into_iter()
            .map(|value| value.id)
            .collect::<Vec<_>>(),
        vec![early.id, late.id]
    );
    assert_eq!(
        repo.list_armed_for_delegated_run(&delegated_run_id)
            .await
            .unwrap()
            .into_iter()
            .map(|value| value.id)
            .collect::<Vec<_>>(),
        vec![early.id, late.id]
    );
    assert!(repo
        .list_armed_for_delegated_run(&AgentRunId::new())
        .await
        .unwrap()
        .is_empty());

    repo.record_job_settled(&early.id, &AgentRunId::new(), "completed")
        .await
        .unwrap();
    assert!(repo.get(&early.id).await.unwrap().unwrap().jobs[0]
        .settled_status
        .is_none());
    repo.record_job_settled(&early.id, &delegated_run_id, "completed")
        .await
        .unwrap();
    assert_eq!(
        repo.get(&early.id).await.unwrap().unwrap().jobs[0]
            .settled_status
            .as_deref(),
        Some("completed")
    );
}

#[tokio::test]
async fn memory_repository_guards_wake_transitions_and_terminal_queries() {
    let repo = MemoryDelegationParkRepo::new();
    let missing = DelegationParkId::new();
    assert!(!repo.claim_wake(&missing, 1).await.unwrap());
    assert!(!repo.reset_wake_claim(&missing).await.unwrap());
    assert!(matches!(
        repo.record_wake_failure(&missing, "missing")
            .await
            .unwrap_err(),
        crate::error::AppError::NotFound(_)
    ));
    repo.settle(&missing, DelegationParkState::Failed, Some("ignored"))
        .await
        .unwrap();
    assert!(repo.get(&missing).await.unwrap().is_none());

    let now = Utc::now();
    let conversation_id = ChatConversationId::new();
    let expired = park(conversation_id.clone(), 7, now - Duration::minutes(2));
    let pending = park(conversation_id.clone(), 8, now + Duration::minutes(2));
    let other = park(ChatConversationId::new(), 9, now - Duration::minutes(1));
    for value in [&expired, &pending, &other] {
        repo.arm((*value).clone()).await.unwrap();
    }

    assert!(!repo.claim_wake(&expired.id, 6).await.unwrap());
    assert!(repo.claim_wake(&expired.id, 7).await.unwrap());
    assert!(!repo.claim_wake(&expired.id, 7).await.unwrap());
    assert_eq!(
        repo.list_wake_stalled(Utc::now() + Duration::seconds(1))
            .await
            .unwrap()[0]
            .id,
        expired.id
    );
    assert_eq!(
        repo.record_wake_failure(&expired.id, "enqueue failed")
            .await
            .unwrap(),
        1
    );
    assert!(repo.reset_wake_claim(&expired.id).await.unwrap());
    assert_eq!(
        repo.get(&expired.id).await.unwrap().unwrap().wake_attempts,
        0
    );
    assert!(!repo.reset_wake_claim(&expired.id).await.unwrap());
    assert_eq!(
        repo.list_expired(now)
            .await
            .unwrap()
            .into_iter()
            .map(|value| value.id)
            .collect::<Vec<_>>(),
        vec![expired.id, other.id]
    );
    assert_eq!(
        repo.supersede_for_conversation(&conversation_id)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        repo.supersede_for_conversation(&conversation_id)
            .await
            .unwrap(),
        0
    );
    assert_eq!(repo.list_armed().await.unwrap().len(), 1);

    assert!(repo.claim_wake(&other.id, other.generation).await.unwrap());
    assert_eq!(
        repo.record_wake_failure(&other.id, "retry").await.unwrap(),
        1
    );
    repo.settle(
        &other.id,
        DelegationParkState::Failed,
        Some("dispatch failed"),
    )
    .await
    .unwrap();
    let failed = repo.get(&other.id).await.unwrap().unwrap();
    assert_eq!(failed.state, DelegationParkState::Failed);
    assert_eq!(failed.wake_attempts, 1);
    assert_eq!(failed.last_error.as_deref(), Some("dispatch failed"));
    assert!(repo.list_wake_stalled(Utc::now()).await.unwrap().is_empty());
}
