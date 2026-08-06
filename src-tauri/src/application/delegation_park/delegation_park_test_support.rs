use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use ralphx_events::RecordingEventSink;
use tokio::sync::Mutex;

use crate::application::chat_service::MockChatService;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{
    AgentRun, AgentRunId, AgentRunStatus, ChatConversation, ChatConversationId, DelegationPark,
    DelegationParkId, DelegationParkJob, DelegationParkState, DelegationWakePolicy, ProjectId,
};
use crate::domain::repositories::{
    AgentRunRepository, ChatConversationRepository, DelegationParkRepository,
};
use crate::error::AppResult;
use crate::infrastructure::agents::claude::delegation_config;
use crate::infrastructure::memory::{MemoryAgentRunRepository, MemoryChatConversationRepository};

use super::{DelegationJobSource, DelegationParkService, ParkJobSnapshot};

/// Stand-in for the transport-owned job registry; the production `DelegationService` mapping is
/// covered by `http_server::delegation_tests` and the `delegation_park` handler suite.
pub(super) struct FakeDelegationJobs {
    job_id: String,
    snapshot: ParkJobSnapshot,
}

impl FakeDelegationJobs {
    pub(super) fn running(
        job_id: &str,
        parent_conversation_id: &ChatConversationId,
        parent_agent_run_id: &AgentRunId,
        delegated_agent_run_id: Option<&AgentRunId>,
    ) -> Self {
        Self {
            job_id: job_id.to_string(),
            snapshot: ParkJobSnapshot {
                status: "running".to_string(),
                parent_conversation_id: Some(parent_conversation_id.as_str()),
                parent_agent_run_id: Some(parent_agent_run_id.as_str()),
                delegated_session_id: "session".to_string(),
                delegated_agent_run_id: delegated_agent_run_id.map(AgentRunId::as_str),
            },
        }
    }
}

#[async_trait]
impl DelegationJobSource for FakeDelegationJobs {
    async fn park_job_snapshot(&self, job_id: &str) -> Option<ParkJobSnapshot> {
        (job_id == self.job_id).then(|| self.snapshot.clone())
    }
}

#[derive(Default)]
pub(super) struct MemoryParkRepository {
    pub(super) parks: Mutex<Vec<DelegationPark>>,
    pub(super) claim_result: AtomicBool,
    pub(super) fail_get: AtomicBool,
    pub(super) supersede_on_get: AtomicBool,
    pub(super) supersede_count: Mutex<usize>,
    pub(super) settled: Mutex<Vec<DelegationParkState>>,
}

impl MemoryParkRepository {
    pub(super) async fn insert(&self, park: DelegationPark) {
        self.parks.lock().await.push(park);
    }
}

#[async_trait]
impl DelegationParkRepository for MemoryParkRepository {
    async fn arm(&self, park: DelegationPark) -> AppResult<DelegationPark> {
        self.parks.lock().await.push(park.clone());
        Ok(park)
    }

    async fn get(&self, id: &DelegationParkId) -> AppResult<Option<DelegationPark>> {
        if self.fail_get.load(Ordering::SeqCst) {
            return Err(crate::error::AppError::Infrastructure(
                "injected delegation park read failure".to_string(),
            ));
        }
        let mut parks = self.parks.lock().await;
        if self.supersede_on_get.swap(false, Ordering::SeqCst) {
            if let Some(park) = parks.iter_mut().find(|park| &park.id == id) {
                park.state = DelegationParkState::Superseded;
                park.updated_at = Utc::now();
            }
        }
        Ok(parks.iter().find(|park| &park.id == id).cloned())
    }

    async fn get_armed_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<DelegationPark>> {
        Ok(self
            .parks
            .lock()
            .await
            .iter()
            .find(|park| {
                park.parent_conversation_id == *conversation_id
                    && park.state == DelegationParkState::Armed
            })
            .cloned())
    }

    async fn get_settlement_blocking_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<DelegationPark>> {
        Ok(self
            .parks
            .lock()
            .await
            .iter()
            .filter(|park| {
                park.parent_conversation_id == *conversation_id
                    && matches!(
                        park.state,
                        DelegationParkState::Armed
                            | DelegationParkState::Waking
                            | DelegationParkState::Woken
                    )
            })
            .max_by_key(|park| park.updated_at)
            .cloned())
    }

    async fn list_armed(&self) -> AppResult<Vec<DelegationPark>> {
        Ok(self
            .parks
            .lock()
            .await
            .iter()
            .filter(|park| park.state == DelegationParkState::Armed)
            .cloned()
            .collect())
    }

    async fn list_armed_for_delegated_run(
        &self,
        run_id: &AgentRunId,
    ) -> AppResult<Vec<DelegationPark>> {
        Ok(self
            .parks
            .lock()
            .await
            .iter()
            .filter(|park| {
                park.state == DelegationParkState::Armed
                    && park
                        .jobs
                        .iter()
                        .any(|job| &job.delegated_agent_run_id == run_id)
            })
            .cloned()
            .collect())
    }

    async fn record_job_settled(
        &self,
        id: &DelegationParkId,
        run_id: &AgentRunId,
        status: &str,
    ) -> AppResult<()> {
        if let Some(park) = self
            .parks
            .lock()
            .await
            .iter_mut()
            .find(|park| &park.id == id)
        {
            if let Some(job) = park
                .jobs
                .iter_mut()
                .find(|job| &job.delegated_agent_run_id == run_id)
            {
                job.settled_status = Some(status.to_string());
            }
        }
        Ok(())
    }

    async fn claim_wake(&self, id: &DelegationParkId, generation: i64) -> AppResult<bool> {
        if !self.claim_result.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(false);
        }
        let mut parks = self.parks.lock().await;
        let Some(park) = parks.iter_mut().find(|park| &park.id == id) else {
            return Ok(false);
        };
        if park.state != DelegationParkState::Armed || park.generation != generation {
            return Ok(false);
        }
        let claimed_at = Utc::now();
        park.state = DelegationParkState::Waking;
        park.wake_claimed_at = Some(claimed_at);
        park.updated_at = claimed_at;
        Ok(true)
    }

    async fn record_wake_failure(&self, id: &DelegationParkId, error: &str) -> AppResult<i32> {
        let mut parks = self.parks.lock().await;
        let Some(park) = parks.iter_mut().find(|park| &park.id == id) else {
            return Err(crate::error::AppError::NotFound(format!(
                "delegation park not found: {id}"
            )));
        };
        park.wake_attempts += 1;
        park.last_error = Some(error.to_string());
        park.updated_at = Utc::now();
        Ok(park.wake_attempts)
    }

    async fn list_wake_stalled(
        &self,
        older_than: chrono::DateTime<Utc>,
    ) -> AppResult<Vec<DelegationPark>> {
        Ok(self
            .parks
            .lock()
            .await
            .iter()
            .filter(|park| {
                park.state == DelegationParkState::Waking && park.updated_at <= older_than
            })
            .cloned()
            .collect())
    }

    async fn reset_wake_claim(&self, id: &DelegationParkId) -> AppResult<bool> {
        let mut parks = self.parks.lock().await;
        let Some(park) = parks.iter_mut().find(|park| &park.id == id) else {
            return Ok(false);
        };
        if park.state != DelegationParkState::Waking {
            return Ok(false);
        }
        park.state = DelegationParkState::Armed;
        park.wake_claimed_at = None;
        park.wake_attempts = 0;
        park.updated_at = Utc::now();
        Ok(true)
    }

    async fn settle(
        &self,
        id: &DelegationParkId,
        state: DelegationParkState,
        error: Option<&str>,
    ) -> AppResult<()> {
        let mut transitioned = false;
        if let Some(park) = self
            .parks
            .lock()
            .await
            .iter_mut()
            .find(|park| &park.id == id && park.state == DelegationParkState::Waking)
        {
            park.state = state;
            park.last_error = error.map(str::to_string);
            transitioned = true;
        }
        if transitioned {
            self.settled.lock().await.push(state);
        }
        Ok(())
    }

    async fn disarm_armed_for_parent_run(
        &self,
        conversation_id: &ChatConversationId,
        parent_run_id: &AgentRunId,
    ) -> AppResult<usize> {
        let mut count = 0;
        for park in self.parks.lock().await.iter_mut() {
            if park.parent_conversation_id == *conversation_id
                && park.parent_agent_run_id == *parent_run_id
                && park.state == DelegationParkState::Armed
            {
                park.state = DelegationParkState::Disarmed;
                park.updated_at = Utc::now();
                count += 1;
            }
        }
        Ok(count)
    }

    async fn supersede_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<usize> {
        *self.supersede_count.lock().await += 1;
        let mut count = 0;
        for park in self.parks.lock().await.iter_mut() {
            if park.parent_conversation_id == *conversation_id
                && matches!(
                    park.state,
                    DelegationParkState::Armed | DelegationParkState::Waking
                )
            {
                park.state = DelegationParkState::Superseded;
                count += 1;
            }
        }
        Ok(count)
    }

    async fn list_expired(&self, _now: chrono::DateTime<Utc>) -> AppResult<Vec<DelegationPark>> {
        Ok(Vec::new())
    }
}

pub(super) struct Harness {
    pub(super) parks: Arc<MemoryParkRepository>,
    pub(super) runs: Arc<MemoryAgentRunRepository>,
    pub(super) conversations: Arc<MemoryChatConversationRepository>,
    pub(super) chat: Arc<MockChatService>,
    pub(super) events: Arc<RecordingEventSink>,
}

impl Harness {
    pub(super) fn service(&self) -> DelegationParkService {
        DelegationParkService::new(
            self.parks.clone(),
            self.chat.clone(),
            self.runs.clone(),
            self.conversations.clone(),
            self.events.clone(),
        )
    }
}

pub(super) fn harness() -> Harness {
    Harness {
        parks: Arc::new(MemoryParkRepository {
            claim_result: AtomicBool::new(true),
            ..Default::default()
        }),
        runs: Arc::new(MemoryAgentRunRepository::new()),
        conversations: Arc::new(MemoryChatConversationRepository::new()),
        chat: Arc::new(MockChatService::new()),
        events: Arc::new(RecordingEventSink::new()),
    }
}

pub(super) async fn insert_parent_and_delegate(
    harness: &Harness,
) -> (ChatConversationId, AgentRun, AgentRun) {
    let conversation = ChatConversation::new_project(ProjectId::from_string("project".to_string()));
    harness
        .conversations
        .create(conversation.clone())
        .await
        .unwrap();
    let mut parent = AgentRun::new(conversation.id.clone());
    parent.status = AgentRunStatus::Completed;
    parent.harness = Some(AgentHarnessKind::Codex);
    parent.logical_model = Some("gpt-5.6".to_string());
    parent.logical_effort = Some(LogicalEffort::High);
    parent.service_tier = Some("priority".to_string());
    harness.runs.create(parent.clone()).await.unwrap();
    let mut delegate = AgentRun::new(ChatConversationId::new());
    delegate.status = AgentRunStatus::Completed;
    delegate.agent_name = Some("researcher".to_string());
    harness.runs.create(delegate.clone()).await.unwrap();
    (conversation.id, parent, delegate)
}

pub(super) fn park(
    conversation_id: ChatConversationId,
    parent_run_id: AgentRunId,
    delegate_run_id: AgentRunId,
) -> DelegationPark {
    let now = Utc::now();
    DelegationPark {
        id: DelegationParkId::new(),
        parent_conversation_id: conversation_id,
        parent_agent_run_id: parent_run_id,
        generation: 1,
        wake_policy: DelegationWakePolicy::AllSettled,
        wake_on_failure: false,
        state: DelegationParkState::Armed,
        deadline_at: now + Duration::seconds(delegation_config().park_max_secs as i64),
        wake_claimed_at: None,
        wake_attempts: 0,
        last_error: None,
        created_at: now,
        updated_at: now,
        jobs: vec![DelegationParkJob {
            job_id: "job".to_string(),
            delegated_session_id: "session".to_string(),
            delegated_agent_run_id: delegate_run_id,
            settled_status: Some("completed".to_string()),
        }],
    }
}
