use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use crate::domain::entities::{RemoteConversationStartRequest, RemoteConversationStartStatus};
use crate::domain::repositories::RemoteConversationStartRequestRepository;
use crate::error::AppResult;

pub struct MemoryRemoteConversationStartRequestRepository {
    requests: Mutex<Vec<RemoteConversationStartRequest>>,
}

impl MemoryRemoteConversationStartRequestRepository {
    pub fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl Default for MemoryRemoteConversationStartRequestRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RemoteConversationStartRequestRepository for MemoryRemoteConversationStartRequestRepository {
    async fn create_start_request(
        &self,
        request: RemoteConversationStartRequest,
    ) -> AppResult<RemoteConversationStartRequest> {
        let mut requests = self.requests.lock().await;
        requests.push(request.clone());
        Ok(request)
    }

    async fn get_start_request(
        &self,
        id: &str,
    ) -> AppResult<Option<RemoteConversationStartRequest>> {
        let requests = self.requests.lock().await;
        Ok(requests.iter().find(|request| request.id == id).cloned())
    }

    async fn claim_pending_start_request(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteConversationStartRequest>> {
        // Held across scan + flip → atomic; insertion order preserves oldest-first.
        let mut requests = self.requests.lock().await;
        let Some(request) = requests
            .iter_mut()
            .find(|request| request.status == RemoteConversationStartStatus::Pending)
        else {
            return Ok(None);
        };
        request.status = RemoteConversationStartStatus::Starting;
        request.claimed_at = Some(claimed_at);
        request.updated_at = claimed_at;
        Ok(Some(request.clone()))
    }

    async fn complete_start_request(
        &self,
        id: &str,
        agent_run_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let mut requests = self.requests.lock().await;
        if let Some(request) = requests.iter_mut().find(|request| {
            request.id == id && request.status == RemoteConversationStartStatus::Starting
        }) {
            request.status = RemoteConversationStartStatus::Started;
            request.agent_run_id = Some(agent_run_id.to_string());
            request.updated_at = updated_at;
        }
        Ok(())
    }

    async fn fail_start_request(
        &self,
        id: &str,
        error_code: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let mut requests = self.requests.lock().await;
        if let Some(request) = requests.iter_mut().find(|request| {
            request.id == id && request.status == RemoteConversationStartStatus::Starting
        }) {
            request.status = RemoteConversationStartStatus::Failed;
            request.error_code = Some(error_code.to_string());
            request.updated_at = updated_at;
        }
        Ok(())
    }

    async fn cancel_pending_start_requests_for_device(
        &self,
        device_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let mut requests = self.requests.lock().await;
        let mut changed = 0u64;
        for request in requests.iter_mut() {
            if request.requested_by_device_id == device_id
                && request.status == RemoteConversationStartStatus::Pending
            {
                request.status = RemoteConversationStartStatus::Cancelled;
                request.updated_at = updated_at;
                changed += 1;
            }
        }
        Ok(changed)
    }

    async fn fail_stale_starting_start_requests(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let mut requests = self.requests.lock().await;
        let mut changed = 0u64;
        for request in requests.iter_mut() {
            if request.status == RemoteConversationStartStatus::Starting
                && request
                    .claimed_at
                    .map(|value| value < claimed_before)
                    .unwrap_or(false)
            {
                request.status = RemoteConversationStartStatus::FailedStale;
                request.updated_at = updated_at;
                changed += 1;
            }
        }
        Ok(changed)
    }
}
