use chrono::{Duration, Utc};

use crate::domain::entities::{
    AgentRunId, ChatConversationId, DelegationPark, DelegationParkId, DelegationParkJob,
    DelegationParkState, DelegationWakePolicy,
};
use crate::domain::repositories::DelegationParkRepository;

use super::MemoryDelegationParkRepo;

fn park() -> DelegationPark {
    let now = Utc::now();
    DelegationPark {
        id: DelegationParkId::new(),
        parent_conversation_id: ChatConversationId::new(),
        parent_agent_run_id: AgentRunId::new(),
        generation: 1,
        wake_policy: DelegationWakePolicy::AllSettled,
        wake_on_failure: true,
        state: DelegationParkState::Armed,
        deadline_at: now + Duration::hours(1),
        wake_claimed_at: None,
        wake_attempts: 0,
        last_error: None,
        created_at: now,
        updated_at: now,
        jobs: vec![DelegationParkJob {
            job_id: "job".to_string(),
            delegated_session_id: "session".to_string(),
            delegated_agent_run_id: AgentRunId::new(),
            settled_status: None,
        }],
    }
}

#[tokio::test]
async fn late_settle_does_not_overwrite_a_superseded_park() {
    let repo = MemoryDelegationParkRepo::new();
    let park = park();
    repo.arm(park.clone()).await.unwrap();
    assert!(repo.claim_wake(&park.id, park.generation).await.unwrap());
    assert_eq!(
        repo.supersede_for_conversation(&park.parent_conversation_id)
            .await
            .unwrap(),
        1
    );

    repo.settle(&park.id, DelegationParkState::Expired, None)
        .await
        .unwrap();

    assert_eq!(
        repo.get(&park.id).await.unwrap().unwrap().state,
        DelegationParkState::Superseded
    );
}
