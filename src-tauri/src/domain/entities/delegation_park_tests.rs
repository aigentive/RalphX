use chrono::{Duration, Utc};

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
