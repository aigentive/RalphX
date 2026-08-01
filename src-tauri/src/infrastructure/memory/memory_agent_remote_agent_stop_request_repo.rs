use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use crate::domain::entities::{ChatConversationId, RemoteAgentStopRequest, RemoteAgentStopStatus};
use crate::domain::repositories::RemoteAgentStopRequestRepository;
use crate::error::AppResult;

pub struct MemoryRemoteAgentStopRequestRepository {
    requests: Mutex<Vec<RemoteAgentStopRequest>>,
}

impl MemoryRemoteAgentStopRequestRepository {
    pub fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl Default for MemoryRemoteAgentStopRequestRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RemoteAgentStopRequestRepository for MemoryRemoteAgentStopRequestRepository {
    async fn create_stop_request(
        &self,
        request: RemoteAgentStopRequest,
    ) -> AppResult<RemoteAgentStopRequest> {
        let mut requests = self.requests.lock().await;
        requests.push(request.clone());
        Ok(request)
    }

    async fn get_stop_request(&self, id: &str) -> AppResult<Option<RemoteAgentStopRequest>> {
        let requests = self.requests.lock().await;
        Ok(requests.iter().find(|request| request.id == id).cloned())
    }

    async fn find_unsettled_stop_request_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<RemoteAgentStopRequest>> {
        let requests = self.requests.lock().await;
        Ok(requests
            .iter()
            .find(|request| {
                &request.conversation_id == conversation_id && !request.status.is_terminal()
            })
            .cloned())
    }

    async fn claim_pending_stop_request(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteAgentStopRequest>> {
        // Held across scan + flip → atomic; insertion order preserves oldest-first.
        let mut requests = self.requests.lock().await;
        let Some(request) = requests
            .iter_mut()
            .find(|request| request.status == RemoteAgentStopStatus::Pending)
        else {
            return Ok(None);
        };
        request.status = RemoteAgentStopStatus::Stopping;
        request.claimed_at = Some(claimed_at);
        request.updated_at = claimed_at;
        Ok(Some(request.clone()))
    }

    async fn complete_stop_request(&self, id: &str, updated_at: DateTime<Utc>) -> AppResult<()> {
        self.settle(id, RemoteAgentStopStatus::Stopped, None, updated_at)
            .await
    }

    async fn resolve_stop_request_no_live_run(
        &self,
        id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        self.settle(id, RemoteAgentStopStatus::NoLiveRun, None, updated_at)
            .await
    }

    async fn fail_stop_request(
        &self,
        id: &str,
        error_code: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        self.settle(
            id,
            RemoteAgentStopStatus::Failed,
            Some(error_code.to_string()),
            updated_at,
        )
        .await
    }

    async fn cancel_pending_stop_requests_for_device(
        &self,
        device_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let mut requests = self.requests.lock().await;
        let mut changed = 0u64;
        for request in requests.iter_mut() {
            if request.requested_by_device_id == device_id
                && request.status == RemoteAgentStopStatus::Pending
            {
                request.status = RemoteAgentStopStatus::Cancelled;
                request.updated_at = updated_at;
                changed += 1;
            }
        }
        Ok(changed)
    }

    async fn fail_stale_stopping_stop_requests(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let mut requests = self.requests.lock().await;
        let mut changed = 0u64;
        for request in requests.iter_mut() {
            if request.status == RemoteAgentStopStatus::Stopping
                && request
                    .claimed_at
                    .map(|value| value < claimed_before)
                    .unwrap_or(false)
            {
                request.status = RemoteAgentStopStatus::FailedStale;
                request.updated_at = updated_at;
                changed += 1;
            }
        }
        Ok(changed)
    }
}

impl MemoryRemoteAgentStopRequestRepository {
    /// Shared terminal write, guarded on `Stopping` exactly like the SQLite `WHERE` clause: a
    /// late completion must never resurrect or downgrade an already-settled row.
    async fn settle(
        &self,
        id: &str,
        status: RemoteAgentStopStatus,
        error_code: Option<String>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let mut requests = self.requests.lock().await;
        if let Some(request) = requests
            .iter_mut()
            .find(|request| request.id == id && request.status == RemoteAgentStopStatus::Stopping)
        {
            request.status = status;
            request.error_code = error_code;
            request.updated_at = updated_at;
        }
        Ok(())
    }
}
