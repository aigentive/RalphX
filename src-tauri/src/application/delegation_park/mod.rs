//! Durable parent-turn parking for native delegation.

#[cfg(test)]
mod arm_tests;
#[cfg(test)]
mod delegation_park_test_support;
mod job_source;
mod reconcile;
mod wake_dispatch;
#[cfg(test)]
mod wake_dispatch_tests;

use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
};

use chrono::{Duration, Utc};
use ralphx_events::EventSink;

use crate::application::chat_service::ChatService;
use crate::domain::entities::{
    AgentRunId, ChatConversationId, DelegationPark, DelegationParkId, DelegationParkJob,
    DelegationParkState, DelegationWakeDecision, DelegationWakePolicy,
};
use crate::domain::repositories::{
    AgentRunRepository, ChatConversationRepository, DelegationParkRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::delegation_config;

pub use job_source::{DelegationJobSource, ParkJobSnapshot};
pub use reconcile::DelegationParkReconcileSummary;

/// Upper bound on watched jobs per park. The job list arrives from an MCP caller, so it is
/// validated before any per-job work is done and the watch vector is never sized from it.
pub const MAX_PARK_JOB_IDS: usize = 64;

/// Request to durably park a coordinator until delegated jobs require a wake.
#[derive(Debug, Clone)]
pub struct ArmParkRequest {
    pub parent_conversation_id: ChatConversationId,
    pub parent_agent_run_id: AgentRunId,
    pub job_ids: Vec<String>,
    pub wake_policy: DelegationWakePolicy,
    pub wake_on_failure: bool,
    pub max_wait_secs: Option<u64>,
}

/// Application service for durable delegation parks and hidden coordinator wakeups.
pub struct DelegationParkService {
    pub(super) park_repo: Arc<dyn DelegationParkRepository>,
    pub(super) chat_service: Arc<dyn ChatService>,
    pub(super) agent_run_repo: Arc<dyn AgentRunRepository>,
    pub(super) conversation_repo: Arc<dyn ChatConversationRepository>,
    pub(super) events: Arc<dyn EventSink>,
    generation_clock: AtomicI64,
}

impl DelegationParkService {
    /// Create a delegation park service from application-layer dependencies.
    pub fn new(
        park_repo: Arc<dyn DelegationParkRepository>,
        chat_service: Arc<dyn ChatService>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        events: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            park_repo,
            chat_service,
            agent_run_repo,
            conversation_repo,
            events,
            generation_clock: AtomicI64::new(Utc::now().timestamp_micros()),
        }
    }

    /// Validate delegate ownership then arm one durable park for the conversation.
    ///
    /// # Errors
    ///
    /// Returns a validation error for unknown, unowned, non-running, or unbound jobs; returns
    /// repository errors when supersession or persistence fails.
    pub async fn arm(
        &self,
        request: ArmParkRequest,
        delegation_jobs: &dyn DelegationJobSource,
    ) -> AppResult<DelegationPark> {
        if request.job_ids.is_empty() {
            return Err(AppError::Validation(
                "delegation park requires at least one running job".to_string(),
            ));
        }
        if request.job_ids.len() > MAX_PARK_JOB_IDS {
            return Err(AppError::Validation(format!(
                "delegation park accepts at most {MAX_PARK_JOB_IDS} jobs"
            )));
        }

        let mut jobs: Vec<DelegationParkJob> = Vec::new();
        for job_id in &request.job_ids {
            let snapshot = delegation_jobs
                .park_job_snapshot(job_id)
                .await
                .ok_or_else(|| AppError::NotFound(format!("delegation job not found: {job_id}")))?;
            if snapshot.status != "running" {
                return Err(AppError::Validation(format!(
                    "delegation job is not running: {job_id}"
                )));
            }
            if snapshot.parent_conversation_id.as_deref()
                != Some(request.parent_conversation_id.as_str().as_str())
                || snapshot.parent_agent_run_id.as_deref()
                    != Some(request.parent_agent_run_id.as_str().as_str())
            {
                return Err(AppError::Validation(format!(
                    "delegation job is not owned by this parent: {job_id}"
                )));
            }
            let delegated_agent_run_id = snapshot.delegated_agent_run_id.ok_or_else(|| {
                AppError::Validation(format!(
                    "delegation job has no durable delegated run: {job_id}"
                ))
            })?;
            let delegated_agent_run_id =
                AgentRunId::from_str(&delegated_agent_run_id).map_err(|_| {
                    AppError::Validation(format!(
                        "delegation job has an invalid durable delegated run: {job_id}"
                    ))
                })?;
            jobs.push(DelegationParkJob {
                job_id: job_id.clone(),
                delegated_session_id: snapshot.delegated_session_id,
                delegated_agent_run_id,
                settled_status: None,
            });
        }

        let config = delegation_config();
        let max_wait_secs = request
            .max_wait_secs
            .unwrap_or(config.park_max_secs)
            .min(config.park_max_secs);
        let max_wait_secs = i64::try_from(max_wait_secs).map_err(|_| {
            AppError::Validation(
                "delegation park maximum wait exceeds supported duration".to_string(),
            )
        })?;
        let now = Utc::now();
        let park = DelegationPark {
            id: DelegationParkId::new(),
            parent_conversation_id: request.parent_conversation_id.clone(),
            parent_agent_run_id: request.parent_agent_run_id,
            // Timestamp seeding makes generations greater across normal restarts; the atomic
            // increment makes every arm strictly increasing within this process.
            generation: self.generation_clock.fetch_add(1, Ordering::SeqCst),
            wake_policy: request.wake_policy,
            wake_on_failure: request.wake_on_failure,
            state: DelegationParkState::Armed,
            deadline_at: now + Duration::seconds(max_wait_secs),
            wake_claimed_at: None,
            wake_attempts: 0,
            last_error: None,
            created_at: now,
            updated_at: now,
            jobs,
        };

        self.park_repo
            .supersede_for_conversation(&park.parent_conversation_id)
            .await?;
        self.park_repo.arm(park).await
    }

    /// Record one delegated run's terminal status and wake each resulting park when due.
    ///
    /// Callers should log failures and continue terminal settlement; reconciliation is the
    /// durable backstop for a failed live notification.
    pub async fn note_job_settled(
        &self,
        delegated_run_id: &AgentRunId,
        status: &str,
    ) -> AppResult<()> {
        let parks = self
            .park_repo
            .list_armed_for_delegated_run(delegated_run_id)
            .await?;
        for park in parks {
            self.park_repo
                .record_job_settled(&park.id, delegated_run_id, status)
                .await?;
            let Some(updated) = self.park_repo.get(&park.id).await? else {
                return Err(AppError::NotFound(format!(
                    "delegation park not found: {}",
                    park.id
                )));
            };
            if let DelegationWakeDecision::Wake(reason) = updated.wake_decision() {
                self.dispatch_wake(&updated, reason).await?;
            }
        }
        Ok(())
    }
}
