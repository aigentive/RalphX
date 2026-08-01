use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use crate::domain::entities::{RemoteConversationMessageRequest, RemoteConversationMessageStatus};
use crate::domain::repositories::RemoteConversationMessageRequestRepository;
use crate::error::AppResult;

pub struct MemoryRemoteConversationMessageRequestRepository {
    requests: Mutex<Vec<RemoteConversationMessageRequest>>,
}

impl MemoryRemoteConversationMessageRequestRepository {
    pub fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl Default for MemoryRemoteConversationMessageRequestRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RemoteConversationMessageRequestRepository
    for MemoryRemoteConversationMessageRequestRepository
{
    async fn create_message_request(
        &self,
        request: RemoteConversationMessageRequest,
    ) -> AppResult<RemoteConversationMessageRequest> {
        let mut requests = self.requests.lock().await;
        requests.push(request.clone());
        Ok(request)
    }

    async fn get_message_request(
        &self,
        id: &str,
    ) -> AppResult<Option<RemoteConversationMessageRequest>> {
        let requests = self.requests.lock().await;
        Ok(requests.iter().find(|request| request.id == id).cloned())
    }

    async fn claim_pending_message_request(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteConversationMessageRequest>> {
        // Held across scan + flip → atomic; insertion order preserves oldest-first.
        let mut requests = self.requests.lock().await;
        let Some(request) = requests
            .iter_mut()
            .find(|request| request.status == RemoteConversationMessageStatus::Pending)
        else {
            return Ok(None);
        };
        request.status = RemoteConversationMessageStatus::Dispatching;
        request.claimed_at = Some(claimed_at);
        request.updated_at = claimed_at;
        Ok(Some(request.clone()))
    }

    async fn complete_message_request(
        &self,
        id: &str,
        agent_run_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let mut requests = self.requests.lock().await;
        if let Some(request) = requests.iter_mut().find(|request| {
            request.id == id && request.status == RemoteConversationMessageStatus::Dispatching
        }) {
            request.status = RemoteConversationMessageStatus::Dispatched;
            request.agent_run_id = Some(agent_run_id.to_string());
            request.updated_at = updated_at;
        }
        Ok(())
    }

    async fn fail_message_request(
        &self,
        id: &str,
        error_code: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let mut requests = self.requests.lock().await;
        if let Some(request) = requests.iter_mut().find(|request| {
            request.id == id && request.status == RemoteConversationMessageStatus::Dispatching
        }) {
            request.status = RemoteConversationMessageStatus::Failed;
            request.error_code = Some(error_code.to_string());
            request.updated_at = updated_at;
        }
        Ok(())
    }

    async fn cancel_pending_message_requests_for_device(
        &self,
        device_id: &str,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let mut requests = self.requests.lock().await;
        let mut changed = 0u64;
        for request in requests.iter_mut() {
            if request.requested_by_device_id == device_id
                && request.status == RemoteConversationMessageStatus::Pending
            {
                request.status = RemoteConversationMessageStatus::Cancelled;
                request.updated_at = updated_at;
                changed += 1;
            }
        }
        Ok(changed)
    }

    async fn fail_stale_dispatching_message_requests(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let mut requests = self.requests.lock().await;
        let mut changed = 0u64;
        for request in requests.iter_mut() {
            if request.status == RemoteConversationMessageStatus::Dispatching
                && request
                    .claimed_at
                    .map(|value| value < claimed_before)
                    .unwrap_or(false)
            {
                request.status = RemoteConversationMessageStatus::FailedStale;
                request.updated_at = updated_at;
                changed += 1;
            }
        }
        Ok(changed)
    }
}
