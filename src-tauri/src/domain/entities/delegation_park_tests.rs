use chrono::{Duration, Utc};
use uuid::Uuid;

use super::{
    AgentRunId, ChatConversationId, DelegationPark, DelegationParkId, DelegationParkJob,
    DelegationParkState, DelegationWakeDecision, DelegationWakePolicy, DelegationWakeReason,
};

fn park(wake_policy: DelegationWakePolicy, wake_on_failure: bool) -> DelegationPark {
    let now = Utc::now();
    DelegationPark {
        id: DelegationParkId::new(),
        parent_conversation_id: ChatConversationId::new(),
        parent_agent_run_id: AgentRunId::new(),
        generation: 1,
        wake_policy,
        wake_on_failure,
        state: DelegationParkState::Armed,
        deadline_at: now + Duration::minutes(5),
        wake_claimed_at: None,
        wake_attempts: 0,
        last_error: None,
        created_at: now,
        updated_at: now,
        jobs: Vec::new(),
    }
}

fn job(status: Option<&str>) -> DelegationParkJob {
    DelegationParkJob {
        job_id: "job-1".to_string(),
        delegated_session_id: "session-1".to_string(),
        delegated_agent_run_id: AgentRunId::new(),
        settled_status: status.map(str::to_string),
    }
}

#[test]
fn wake_decision_prefers_delegate_failure() {
    let mut park = park(DelegationWakePolicy::AllSettled, true);
    park.jobs = vec![job(Some("completed")), job(Some("failed"))];

    assert_eq!(
        park.wake_decision(),
        DelegationWakeDecision::Wake(DelegationWakeReason::DelegateFailed)
    );
}

#[test]
fn wake_decision_requires_non_empty_all_settled_jobs() {
    let mut park = park(DelegationWakePolicy::AllSettled, false);
    assert_eq!(park.wake_decision(), DelegationWakeDecision::Wait);

    park.jobs = vec![job(Some("completed")), job(None)];
    assert_eq!(park.wake_decision(), DelegationWakeDecision::Wait);

    park.jobs[1].settled_status = Some("cancelled".to_string());
    assert_eq!(
        park.wake_decision(),
        DelegationWakeDecision::Wake(DelegationWakeReason::AllSettled)
    );
}

#[test]
fn wake_decision_wakes_for_any_settled_job() {
    let mut park = park(DelegationWakePolicy::AnySettled, false);
    park.jobs = vec![job(None), job(Some("completed"))];

    assert_eq!(
        park.wake_decision(),
        DelegationWakeDecision::Wake(DelegationWakeReason::AnySettled)
    );
}

#[test]
fn expiry_is_inclusive_of_the_deadline() {
    let mut park = park(DelegationWakePolicy::AllSettled, false);
    let now = Utc::now();
    park.deadline_at = now;

    assert!(park.is_expired(now));
    assert!(!park.is_expired(now - Duration::nanoseconds(1)));
}

#[test]
fn park_id_round_trips_across_storage_and_uuid_boundaries() {
    let uuid = Uuid::new_v4();
    let from_uuid = DelegationParkId::from(uuid);
    let stored = from_uuid.as_str();

    assert_eq!(from_uuid.as_uuid(), &uuid);
    assert_eq!(DelegationParkId::from_string(&stored), from_uuid);
    assert_eq!(stored.parse::<DelegationParkId>().unwrap(), from_uuid);
    assert_eq!(String::from(from_uuid), stored);
    assert_eq!(from_uuid.to_string(), stored);
    assert_eq!(
        DelegationParkId::from_string("not-a-uuid").as_uuid(),
        &Uuid::nil()
    );
    assert!("not-a-uuid".parse::<DelegationParkId>().is_err());
    assert_ne!(DelegationParkId::default(), DelegationParkId::default());
}

#[test]
fn park_state_storage_contract_round_trips_every_variant() {
    let cases = [
        (DelegationParkState::Armed, "armed"),
        (DelegationParkState::Waking, "waking"),
        (DelegationParkState::Woken, "woken"),
        (DelegationParkState::Superseded, "superseded"),
        (DelegationParkState::Expired, "expired"),
        (DelegationParkState::Failed, "failed"),
    ];

    for (state, stored) in cases {
        assert_eq!(state.as_str(), stored);
        assert_eq!(state.to_string(), stored);
        assert_eq!(stored.parse::<DelegationParkState>().unwrap(), state);
    }
    assert_eq!(
        "unknown".parse::<DelegationParkState>().unwrap_err(),
        "Invalid delegation park state: unknown"
    );
}

#[test]
fn wake_policy_storage_contract_round_trips_every_variant() {
    let cases = [
        (DelegationWakePolicy::AllSettled, "all_settled"),
        (DelegationWakePolicy::AnySettled, "any_settled"),
    ];

    for (policy, stored) in cases {
        assert_eq!(policy.as_str(), stored);
        assert_eq!(policy.to_string(), stored);
        assert_eq!(stored.parse::<DelegationWakePolicy>().unwrap(), policy);
    }
    assert_eq!(
        "unknown".parse::<DelegationWakePolicy>().unwrap_err(),
        "Invalid delegation wake policy: unknown"
    );
}

#[test]
fn cancelled_delegate_is_a_failure_wake_when_requested() {
    let mut park = park(DelegationWakePolicy::AllSettled, true);
    park.jobs = vec![job(None), job(Some("cancelled"))];

    assert_eq!(
        park.wake_decision(),
        DelegationWakeDecision::Wake(DelegationWakeReason::DelegateFailed)
    );
}
